use crate::screening_state::{advance_state, get_current_state, ScreeningState};
use crate::types::{
    AnalysisResult, FaceInfo, HeadLockConfidence, LineSegment, Point, Quad, RawShape, Rect,
    SolidShape, Span, ThermalReference,
};
use geo::bounding_rect::BoundingRect;
use geo::contains::Contains;
use geo::convexhull::ConvexHull;
use geo_types::{Coordinate, MultiPoint, MultiPolygon, Rect as GeoRect};
use js_sys::{Float32Array, Uint16Array, Uint8Array};

#[allow(unused)]
use log::{info, trace, warn};
use std::cmp::Ordering;
use std::collections::VecDeque;

use crate::init::{
    ImageBuffers, BACKGROUND_BIT, BODY_AREA_THIS_FRAME, BODY_SHAPE, FACE, FACE_SHAPE, FRAME_NUM,
    HAS_BODY, HEIGHT, IMAGE_BUFFERS, MOTION_BIT, MOTION_BUFFER, THERMAL_REF, WIDTH,
};
use crate::shape_processing::{
    clear_body_shape, clear_face_shape, get_neck, guess_approx_head_width,
};
use crate::smoothing::{median_smooth_pass, radial_smooth_half, rotate};
use crate::thermal_reference::{
    detect_thermal_ref, extract_sensor_value_for_circle, get_extended_thermal_ref_rect_full_clip,
    THERMAL_REF_WIDTH,
};
use geo::Polygon;
use imgref::Img;
use std::iter::FromIterator;
use wasm_bindgen::__rt::core::f32::consts::PI;
use wasm_bindgen::prelude::*;

mod init;
mod screening_state;
mod shape_processing;
mod smoothing;
#[cfg(test)]
mod tests;
mod thermal_reference;
mod types;

// For when we're running this in a web-worker context and Window etc is not available.
#[cfg(not(feature = "perf-profiling"))]
struct Perf {}
#[cfg(not(feature = "perf-profiling"))]
impl Perf {
    pub fn new(_label: &str) -> Option<Perf> {
        None
    }
}

#[cfg(feature = "perf-profiling")]
struct Perf<'a> {
    mark: &'a str,
    performance: web_sys::Performance,
}

#[cfg(feature = "perf-profiling")]
impl<'a> Perf<'a> {
    pub fn new(label: &'a str) -> Option<Perf> {
        match web_sys::window() {
            Some(window) => {
                let performance = window
                    .performance()
                    .expect("performance should be available");
                performance.mark(label).unwrap();
                Some(Perf {
                    mark: label,
                    performance,
                })
            }
            None => None,
        }
    }
}

#[cfg(feature = "perf-profiling")]
impl<'a> Drop for Perf<'a> {
    fn drop(&mut self) {
        self.performance
            .measure_with_start_mark(self.mark, self.mark)
            .unwrap();
    }
}

fn get_threshold_outside_motion(
    motion_shapes: VecDeque<RawShape>,
    radial_smoothed: Img<&[f32]>,
    min_accumulator: Img<&[f32]>,
) -> Option<(f32, f32, f32, Rect, VecDeque<RawShape>, SolidShape)> {
    // Get a convex hull of the motion
    let hull = MultiPoint::from_iter(motion_shapes.iter().flat_map(|shape| {
        shape.inner.iter().filter(|x| x.is_some()).flat_map(|x| {
            let row = x.as_ref().unwrap();
            let first = row.first().unwrap();
            let last = row.last().unwrap();
            vec![
                (first.x0 as f32, first.y as f32),
                (last.x1 as f32 - 1.0, last.y as f32),
            ]
        })
    }))
    .convex_hull();
    // for point in &points.0 {
    //     mask[(point.x() as usize, point.y() as usize)] |= 1 << 4;
    // }
    // Get the convex hull of the shape, and find the hottest point outside the hull, which is also
    // not inside the thermal_ref_rect
    //let hull = points.convex_hull();

    // Maybe we still do an adaptive threshold, but only get the stuff inside the hull?
    match hull.bounding_rect() {
        Some(bounds) => {
            let Coordinate { x: x0, y: y0 } = bounds.min();
            let x0 = x0 as usize;
            let y0 = y0 as usize;
            let width = f32::ceil(bounds.width()) as usize;

            let hull = MultiPolygon::from(vec![hull]);
            let mut motion_hull_shape = get_solid_shapes_for_hull(&hull);
            extend_shape_to_bottom(&mut motion_hull_shape, 0);
            let height = HEIGHT - y0;

            let (min, max) = {
                let _p = Perf::new("min/max");
                radial_smoothed
                    .sub_image(0, y0, WIDTH, height)
                    .rows()
                    .zip(motion_hull_shape.inner.iter())
                    .flat_map(|(row, span)| &row[(span.x0 as usize)..(span.x1 as usize)]) // These offsets will be wrong...
                    .fold((f32::MAX, 0.0), |(min, max), &val| {
                        (f32::min(min, val), f32::max(max, val))
                    })
            };

            let threshold = {
                let _p = Perf::new("Find local threshold");
                let range = (max - min) as f32;

                // Calculate the histogram for the region covered by the convex hull,
                // as well as the area filled
                const NUM_BUCKETS: usize = 16;
                let mut histogram: [u16; NUM_BUCKETS] = [0u16; NUM_BUCKETS];
                for bucket_index in radial_smoothed
                    .sub_image(0, y0, WIDTH, height)
                    .rows()
                    .zip(motion_hull_shape.inner.iter())
                    .flat_map(|(row, span)| &row[(span.x0 as usize)..(span.x1 as usize)])
                    .map(|&val| {
                        // Map the value into its histogram bucket.
                        usize::min(
                            NUM_BUCKETS - 1,
                            f32::floor(((val - min) / range) * (NUM_BUCKETS - 1) as f32) as usize,
                        )
                    })
                {
                    histogram[bucket_index] += 1;
                }

                let b = histogram
                    .windows(2)
                    .enumerate()
                    .map(|(index, window)| (window[1] as i16 - window[0] as i16, index, index + 1))
                    .skip(3) // Never in the first three?  This may depend on distributions.
                    .max_by(|a, b| a.0.cmp(&b.0))
                    .unwrap()
                    .1;
                let t = (min as f32) + (range / histogram.len() as f32) * b as f32;
                let threshold = t;
                // Don't use anything where the total range is under 300, it's too flat to be a person?
                let threshold = if range < 300.0 { max } else { threshold };
                threshold
            };

            // Now get the raw shapes of the pixels above the threshold within the motion convex hull.

            // Seems like we could probably use better boundary tracing algorithm here:

            // TODO(jon): If there are no pixels above a higher threshold (32 degrees?), throw them all away,
            //  since it's probably just background noise.

            const LOWER_BOUND: f32 = 30.0;
            const UPPER_BOUND: f32 = 50.0;
            let mut raw_shapes = VecDeque::with_capacity(motion_hull_shape.len());
            {
                // This is where we might benefit from better border detection...
                let _p = Perf::new("Thresholding");
                let mut highest_temp = 0.0;

                for (row, span) in radial_smoothed
                    .sub_image(0, y0, WIDTH, height) // TODO(jon): Crop to thermal ref rect?
                    .rows()
                    .zip(motion_hull_shape.inner.iter())
                    .map(|(row, span)| (&row[(span.x0 as usize)..(span.x1 as usize)], span))
                {
                    // If an edge starts, start a new span, if an edge ends, end the span
                    // and try to assign it to an existing shape, or start a new shape.
                    let mut prev = 0.0;
                    let mut min_bg_px = 0.0;
                    let mut new_span = None;
                    for (x, &px) in row.iter().enumerate() {
                        highest_temp = f32::max(px, highest_temp);

                        // Or if the pixel fails the min accumulator test?
                        // if the pixel isn't over the min_background threshold, continue.
                        let prev_over_min =
                            prev - min_bg_px > UPPER_BOUND || min_bg_px - prev > LOWER_BOUND;
                        min_bg_px = min_accumulator[(x + span.x0 as usize, span.y as usize)];
                        let over_min = px - min_bg_px > UPPER_BOUND || min_bg_px - px > LOWER_BOUND;

                        if (over_min && !prev_over_min) {
                            //|| px >= threshold && prev < threshold {
                            // Start a span
                            highest_temp = f32::max(px, highest_temp);
                            new_span = Some(Span {
                                x0: x as u8 + span.x0,
                                x1: x as u8 + 1 + span.x0,
                                y: span.y,
                            })
                        } else if (!over_min && prev_over_min) {
                            //|| (px < threshold && prev >= threshold)
                            // End the current span and assign it.
                            if let Some(mut new_span) = new_span.take() {
                                new_span.x1 = x as u8 + span.x0;
                                if new_span.width() > 1 {
                                    new_span.assign_to_shape(&mut raw_shapes);
                                }
                            }
                        }
                        prev = px;
                    }
                    //min_bg_px = min_accumulator[(x - 1, span.y as usize)];
                    let prev_over_min =
                        prev - min_bg_px > UPPER_BOUND || min_bg_px - prev > LOWER_BOUND;

                    if prev_over_min {
                        //|| prev >= threshold {
                        // End the current span and assign it.
                        if let Some(mut new_span) = new_span.take() {
                            new_span.x1 = span.x1;
                            if new_span.width() > 1 {
                                new_span.assign_to_shape(&mut raw_shapes);
                            }
                        }
                    }
                }
                // FIXME(jon): Once we've done the first thresholding pass, threshold *again* within that region
                //  to find the hottest oval, and use that to inform our ideas about where the head is.   Take a hull of it.
                // =========================================================================================================

                let num_raw_shapes = raw_shapes.len();
                for _ in 0..num_raw_shapes {
                    // Take the shape from the front, either discard it or stick it on the back.
                    if let Some(shape) = raw_shapes.pop_front() {
                        // Does shape pass?
                        if keep_shape(&shape, &radial_smoothed) {
                            raw_shapes.push_back(shape);
                        }
                    }
                }
            }

            // TODO(jon): Filter out raw shapes below a certain area, and that are too far away from other shapes.

            Some((
                min,
                max,
                threshold,
                Rect::new(x0, y0, width, height),
                raw_shapes,
                motion_hull_shape,
            ))
        }
        None => None,
    }
}

fn keep_shape(shape: &RawShape, radial_smoothed: &Img<&[f32]>) -> bool {
    // if shape.area() < 40 {
    //     return false;
    // }

    //
    // let mut all_spans_one_wide = true;
    // 'outer: for spans in &shape.inner {
    //     if let Some(spans) = spans {
    //         for span in spans {
    //             if span.width() > 1 {
    //                 all_spans_one_wide = false;
    //                 break 'outer;
    //             }
    //         }
    //     }
    // }
    // if all_spans_one_wide {
    //     return false;
    // }
    //

    /*
    let mut min = f32::MAX;
    let mut max = 0.0;
    const NUM_BUCKETS: usize = 16;
    let mut histogram: [u16; NUM_BUCKETS] = [0u16; NUM_BUCKETS];
    for spans in &shape.inner {
        if let Some(spans) = spans {
            for span in spans {
                for x in span.x0 as usize..span.x1 as usize {
                    let val = radial_smoothed[(x, span.y as usize)];
                    min = f32::min(min, val);
                    max = f32::max(max, val);
                }
            }
        }
    }
    let range = max - min;
    for spans in &shape.inner {
        if let Some(spans) = spans {
            for span in spans {
                for x in span.x0 as usize..span.x1 as usize {
                    let val = radial_smoothed[(x, span.y as usize)];
                    let bucket = usize::min(
                        NUM_BUCKETS - 1,
                        f32::floor(((val - min) / range) * (NUM_BUCKETS - 1) as f32) as usize,
                    );
                    histogram[bucket] += 1;
                }
            }
        }
    }

    let useful_dynamic_range = histogram.iter().filter(|&x| *x > 10).count();
    let dynamic_range = (range / histogram.len() as f32) * useful_dynamic_range as f32;
    // Look at the histogram for each shape, make sure it has enough dynamic range to be considered:
    dynamic_range > 100.0

     */
    true
}

fn get_solid_shapes_for_hull(hull: &MultiPolygon<f32>) -> SolidShape {
    let _p = Perf::new("Rasterizing");

    let bounds = hull.bounding_rect().unwrap();
    let Coordinate { x: x0, y: y0 } = bounds.min();
    let Coordinate { x: x1, y: y1 } = bounds.max();
    let x0 = x0 as isize;
    let y0 = y0 as isize;
    let x1 = x1 as isize;
    let y1 = y1 as isize;

    let mut x_start = x0;
    let mut x_end = x1;
    let search_offset = 3;
    let mut shape = Vec::with_capacity((y1 - y0) as usize);

    let boundary_query = |x, y| hull.contains(&Coordinate::from((x, y)));

    for y in (y0..y1).map(|y| y as f32) {
        // For each row, look for the first and last pixel that falls within the convex hull polygon,
        // starting `search_offset` pixels before the offset found on the previous row, to minimise
        // search comparisons.

        // If finding a border close to the previous rows offset fails, fallback to exhaustive search
        if let (Some(start), Some(end)) = (
            (x0..x1)
                .skip(isize::max(0, x_start - x0 - search_offset) as usize)
                .map(|x| x as f32)
                .find(|&x| boundary_query(x, y))
                .or_else(|| (x0..x1).map(|x| x as f32).find(|&x| boundary_query(x, y))),
            (x0..x1)
                .rev()
                .skip(isize::max(0, x1 - x_end - search_offset) as usize)
                .map(|x| x as f32)
                .find(|&x| boundary_query(x, y))
                .or_else(|| {
                    (x0..x1)
                        .rev()
                        .map(|x| x as f32)
                        .find(|&x| boundary_query(x, y))
                }),
        ) {
            x_start = start as isize;
            x_end = end as isize + 1;
            shape.push(Span {
                x0: x_start as u8,
                x1: x_end as u8,
                y: y as u8,
            });
        }
    }

    SolidShape::from_vec(shape)
}

fn extract_internal(
    motion_shapes: VecDeque<RawShape>,
    image_buffers: &ImageBuffers,
    thermal_ref_rect: Rect,
    motion_for_frame: usize,
) -> AnalysisResult {
    let mut analysis_result = AnalysisResult::default();
    let radial_smoothed = &image_buffers.radial_smoothed.borrow();
    let median_smoothed = &image_buffers.median_smoothed.borrow();
    let min_accumulator = &image_buffers.min_accumulator.borrow();
    let mask = &mut image_buffers.mask.borrow_mut();
    {
        let _p = Perf::new("Global min/max");
        let (min, max) = radial_smoothed
            .pixels()
            .fold((f32::MAX, 0.0), |(min, max), val| {
                (f32::min(val, min), f32::max(val, max))
            });
        analysis_result.heat_stats.min = min as u16;
        analysis_result.heat_stats.max = max as u16;
    }

    let (_, _, threshold, _motion_hull_bounds, threshold_raw_shapes, motion_hull_shape) =
        get_threshold_outside_motion(
            motion_shapes,
            radial_smoothed.as_ref(),
            min_accumulator.as_ref(),
        )
        .unwrap_or((
            0.0,
            0.0,
            0.0, // Should use previous frames threshold!
            Rect::default(),
            VecDeque::new(),
            SolidShape::new(),
        ));

    #[cfg(feature = "output-mask-shapes")]
    {
        info!("Output masks");
        for span in &motion_hull_shape.inner {
            let y = span.y as usize;
            for x in span.x0..span.x1 {
                mask[(x as usize, y)] |= 1 << 7;
            }
        }

        for shape in &threshold_raw_shapes {
            for row in &shape.inner {
                if let Some(row) = row {
                    for &span in row {
                        let y = span.y as usize;
                        for x in span.x0..span.x1 {
                            mask[(x as usize, y)] |= 1 << 4;
                        }
                    }
                }
            }
        }
    }

    // Make sure we zero out the thermal reference again? - maybe not necessary if not using solidShapes for threshold
    analysis_result.heat_stats.threshold = threshold as u16;
    // Count up the action of pixels.
    analysis_result.motion_sum = motion_for_frame as u16;
    analysis_result.motion_threshold_sum =
        threshold_raw_shapes.iter().map(|shape| shape.area()).sum();
    analysis_result.frame_bottom_sum = match motion_hull_shape.inner.last() {
        Some(last_span) => {
            if last_span.y >= HEIGHT as u8 / 2 {
                1
            } else {
                0
            }
        }
        _ => 0,
    };
    let has_body = threshold_raw_shapes.len() > 0
        && analysis_result.frame_bottom_sum != 0
        && analysis_result.motion_threshold_sum > 45;

    // Yep, I think we can skip some steps here, doing away with a lot of intermediate mask filling.
    if has_body {
        let _p = Perf::new("Refining head");
        // Do more work to isolate the threshold shapes we care about
        let (point_cloud, hull) = refine_threshold_data(&threshold_raw_shapes);
        // Get the bounds of the point cloud.
        if let Some(bounds) = hull.bounding_rect() {
            // Here we basically just want to trim threshold_raw_shapes to the convex hull we just made,
            // and fill in the gaps in the raw shapes:

            //let mut solid_shapes = get_solid_shapes_from_hull(&b_hull, mask, THRESHOLD_BIT);
            let mut solid_shapes =
                get_solid_shapes_from_hull_2(&hull, &bounds, &threshold_raw_shapes);
            solid_shapes.sort_by(|a, b| a.area().cmp(&b.area()));
            let largest_shape = solid_shapes.pop();

            // Merge shapes if they are clearly the same shape:
            if let Some(mut body_shape) = largest_shape {
                analysis_result.has_body = true;
                body_shape = merge_shapes(body_shape, solid_shapes);
                // Fill vertical cracks in body

                #[cfg(feature = "output-mask-shapes")]
                {
                    let mut raw_shape = RawShape::new();
                    for span in &body_shape.inner {
                        let y = span.y as usize;

                        if let Some(spans) = &mut raw_shape.inner[y] {
                            spans[0].x0 = u8::min(spans[0].x0, span.x0);
                            spans[0].x1 = u8::max(spans[0].x1, span.x1);
                        } else {
                            raw_shape.add_span(span.clone());
                        }
                    }
                    for row in &raw_shape.inner {
                        if let Some(row) = row {
                            let y = row[0].y as usize;
                            for x in row[0].x0..row[0].x1 {
                                mask[(x as usize, y)] |= 1 << 3;
                            }
                        }
                    }
                }
                fill_vertical_cracks(&mut body_shape);
                #[cfg(feature = "output-mask-shapes")]
                {
                    let mut raw_shape = RawShape::new();
                    for span in &body_shape.inner {
                        let y = span.y as usize;

                        if let Some(spans) = &mut raw_shape.inner[y] {
                            spans[0].x0 = u8::min(spans[0].x0, span.x0);
                            spans[0].x1 = u8::max(spans[0].x1, span.x1);
                        } else {
                            raw_shape.add_span(span.clone());
                        }
                    }
                    for row in &raw_shape.inner {
                        if let Some(row) = row {
                            let y = row[0].y as usize;
                            for x in row[0].x0..row[0].x1 {
                                mask[(x as usize, y)] |= 1 << 2;
                            }
                        }
                    }
                }

                {
                    // If any shapes that border an edge have chips out of them along that edge, fill the chips.
                    // |
                    // >  <-- chip
                    // |
                    let thermal_reference_is_on_left = thermal_ref_rect.x1 < WIDTH / 2;
                    let (left, right) = if thermal_reference_is_on_left {
                        ((thermal_ref_rect.x1 + 2) as u8, WIDTH as u8)
                    } else {
                        (0u8, (thermal_ref_rect.x0 - 2) as u8)
                    };

                    let mut prev_span: Option<Span> = None;
                    let mut fill_start_left_y: Option<usize> = None;
                    let mut fill_start_right_y: Option<usize> = None;
                    let mut left_fills = Vec::new();
                    let mut right_fills = Vec::new();
                    for (index, span) in &mut body_shape.inner.iter().enumerate() {
                        if let Some(prev_span) = prev_span {
                            if prev_span.x0 == left && span.x0 != left {
                                fill_start_left_y = Some(index);
                            }
                            if prev_span.x1 == right && span.x1 != right {
                                fill_start_right_y = Some(index);
                            }
                            if span.x0 == left && fill_start_left_y.is_some() {
                                if index - fill_start_left_y.unwrap() < 15 {
                                    left_fills.push((fill_start_left_y.unwrap(), index));
                                }
                                fill_start_left_y = None;
                            }
                            if span.x1 == right && fill_start_right_y.is_some() {
                                if index - fill_start_right_y.unwrap() < 15 {
                                    right_fills.push((fill_start_right_y.unwrap(), index));
                                }
                                fill_start_right_y = None;
                            }
                        }
                        prev_span = Some(span.clone());
                    }
                    for (start, end) in left_fills {
                        for index in start..end {
                            body_shape.inner[index].x0 = left;
                        }
                    }
                    for (start, end) in right_fills {
                        for index in start..end {
                            body_shape.inner[index].x1 = right;
                        }
                    }
                    for row in &mut body_shape.inner {
                        row.x1 = u8::min(WIDTH as u8, row.x1);
                    }
                }

                // Another way of doing this would be to look at vertical slices from left to right, and find the major discontinuities there.

                let approx_head_width = guess_approx_head_width(body_shape.clone());

                // Get a threshold from the hottest parts of body_shape?
                #[cfg(feature = "face-thresholding")]
                {
                    const NUM_BUCKETS: usize = 16;
                    let mut histogram: [u16; NUM_BUCKETS] = [0u16; NUM_BUCKETS];
                    let y0 = body_shape.inner[0].y as usize;
                    let (min, max) = median_smoothed
                        .sub_image(0, y0, WIDTH, HEIGHT - y0)
                        .rows()
                        .zip(body_shape.inner.iter())
                        .flat_map(|(row, span)| &row[(span.x0 as usize)..(span.x1 as usize)])
                        .fold((f32::MAX, 0.0f32), |acc, &b| {
                            (f32::min(acc.0, b), f32::max(acc.1, b))
                        });
                    let range = max - min;
                    for bucket_index in median_smoothed
                        .sub_image(0, y0, WIDTH, HEIGHT - y0)
                        .rows()
                        .zip(body_shape.inner.iter())
                        .flat_map(|(row, span)| &row[(span.x0 as usize)..(span.x1 as usize)])
                        .map(|&val| {
                            // Map the value into its histogram bucket.
                            usize::min(
                                NUM_BUCKETS - 1,
                                f32::floor(((val - min) / range) * (NUM_BUCKETS - 1) as f32)
                                    as usize,
                            )
                        })
                    {
                        histogram[bucket_index] += 1;
                    }
                    //if get_frame_num() == 95 {
                    let mapping = histogram
                        .iter()
                        .enumerate()
                        .map(|(index, &v)| {
                            let t = (min as f32) + (range / histogram.len() as f32) * v as f32;
                            (index, v, t)
                        })
                        .collect::<Vec<_>>();

                    //info!("#{} Hist {:#?}", get_frame_num(), mapping);
                    let approx_head_width = approx_head_width as u16;
                    // If there's a face, we want at least the top ~700 pixels to be inside our threshold.
                    let expected_face_area =
                        u16::max(700, ((approx_head_width / 2) * (approx_head_width / 2)) * 4);

                    // TODO(jon): Detect if the person is wearing glasses or not (threshold big black areas in center of where
                    // we think the face is.
                    // If not wearing glasses, look for the inner canthus points, and use that as our frame of reference.
                    // Glasses can be 3.5 degrees cooler than the face often.

                    let (index, a) =
                        histogram
                            .iter()
                            .enumerate()
                            .rev()
                            .fold((None, 0), |acc, (i, &v)| {
                                if acc.1 + v < expected_face_area {
                                    (Some(i), acc.1 + v)
                                } else {
                                    (acc.0, acc.1 + v)
                                }
                            });
                    let face_threshold = if let Some(index) = index {
                        (min as f32) + (range / histogram.len() as f32) * index as f32
                    } else {
                        max
                    };
                    // let face_threshold = (min as f32)
                    //     + (range / histogram.len() as f32) * (histogram.len() - 1) as f32;
                    // info!("#{}, face threshold {}", get_frame_num(), face_threshold);
                    //let face_threshold = max as f32 - 15.0;

                    let mut raw_face: VecDeque<RawShape> =
                        VecDeque::with_capacity(body_shape.len());
                    let y0 = body_shape.inner.first().map_or(0, |span| span.y) as usize;
                    {
                        // This is where we might benefit from better border detection...
                        let _p = Perf::new("Thresholding face");

                        // TODO(jon): One idea is to keep thresholding the face as long as we have
                        //  something that is the right ratio/has the right centroid placement.

                        // TODO(jon): When thresholding for silhouettes, we'd like to start from the bottom,
                        // and work our way up, so that we include the torso, but not the background.

                        for (row, span) in median_smoothed
                            .sub_image(0, y0, WIDTH, HEIGHT - y0)
                            .rows()
                            .zip(body_shape.inner.iter())
                            .map(|(row, span)| (&row[(span.x0 as usize)..(span.x1 as usize)], span))
                        {
                            // If an edge starts, start a new span, if an edge ends, end the span
                            // and try to assign it to an existing shape, or start a new shape.
                            let mut prev = 0.0;
                            let mut new_span = None;
                            for (x, &px) in row.iter().enumerate() {
                                if px >= face_threshold && prev < face_threshold {
                                    // Start a span
                                    new_span = Some(Span {
                                        x0: x as u8 + span.x0,
                                        x1: x as u8 + 1 + span.x0,
                                        y: span.y,
                                    })
                                } else if px < face_threshold && prev >= face_threshold {
                                    // End the current span and assign it.
                                    if let Some(mut new_span) = new_span.take() {
                                        new_span.x1 = x as u8 + span.x0;
                                        new_span.assign_to_shape(&mut raw_face);
                                    }
                                }
                                prev = px;
                            }
                            if prev >= face_threshold {
                                // End the current span and assign it.
                                if let Some(mut new_span) = new_span.take() {
                                    new_span.x1 = span.x1;
                                    new_span.assign_to_shape(&mut raw_face);
                                }
                            }
                        }
                    }

                    // Filter out any shapes below a certain size:
                    let num_raw_shapes = raw_face.len();
                    for _ in 0..num_raw_shapes {
                        // Take the shape from the front, either discard it or stick it on the back.
                        if let Some(shape) = raw_face.pop_front() {
                            // Does shape pass?
                            if shape.area() > 150 {
                                //let aabb_center = shape.bounds().centroid();
                                //let centroid = shape.centroid();

                                // Remove shapes that aren't shaped like faces (ish),
                                // probably still want a pass to remove far-way shapes?
                                //if aabb_center.distance_to(centroid) < 3.0 {
                                // FIXME(jon): Isn't it really the centroid of the *hull* we want,
                                //  not of the individual shapes that make it up?
                                raw_face.push_back(shape);
                            }
                            //}
                        }
                    }

                    // Filter out shapes that are clearly not connected:
                    // For the face, we're mostly interested in something whose centroid is close
                    // to the center of its aabb.
                    if raw_face.len() != 0 {
                        let face_hull = MultiPoint::from_iter(
                            raw_face // reduce_points
                                .iter()
                                .flat_map(|x| {
                                    x.inner.iter().filter(|x| x.is_some()).map(|x| {
                                        let row = x.as_ref().unwrap();
                                        let first = row.first().unwrap();
                                        let last = row.last().unwrap();
                                        vec![
                                            (first.x0 as f32, first.y as f32),
                                            (last.x1 as f32, last.y as f32),
                                        ]
                                    })
                                })
                                .flatten(),
                        )
                        .convex_hull();
                        // Now rasterise the hull:

                        let solid_face =
                            get_solid_shapes_for_hull(&MultiPolygon::from(vec![face_hull]));

                        FACE_SHAPE.with(|arr_ref| {
                            let mut face_outline = arr_ref.borrow_mut();
                            while face_outline.len() != 0 {
                                face_outline.pop();
                            }
                            for span in &solid_face.inner {
                                face_outline.push(span.y);
                                face_outline.push(span.x0);
                                face_outline.push(span.x1);
                            }
                        });
                    } else {
                        FACE_SHAPE.with(|arr_ref| {
                            let mut face_outline = arr_ref.borrow_mut();
                            while face_outline.len() != 0 {
                                face_outline.pop();
                            }
                        });
                    }

                    // Take a hull of the raw face.
                    let mut a = 0;
                    for shape in &raw_face {
                        for row in &shape.inner {
                            if let Some(row) = row {
                                for &span in row {
                                    let y = span.y as usize;
                                    for x in span.x0..span.x1 {
                                        a += 1;
                                        mask[(x as usize, y)] |= 1 << 5;
                                    }
                                }
                            }
                        }
                    }
                    // info!(
                    //     "#{}, w {}, expected {}, got {} ~ {} ... {}",
                    //     get_frame_num(),
                    //     approx_head_width,
                    //     expected_face_area,
                    //     a,
                    //     (((approx_head_width / 2) * (approx_head_width / 2)) as f32 * 3.5) as u16,
                    //     (approx_head_width / 2) * (approx_head_width / 2)
                    // );
                    //info!("#{}, {} area", get_frame_num(), a);
                    //

                    // Draw it.
                }

                // let (distance_from_left_of_top_to_edge, distance_from_right_of_top_to_edge) =
                //     if body_shape.len() > 0 {
                //         (body_shape.inner[0].x0, WIDTH as u8 - body_shape.inner[0].x1)
                //     } else {
                //         (0, 0)
                //     };
                if approx_head_width > 0
                //&& distance_from_left_of_top_to_edge >= 5
                //&& distance_from_right_of_top_to_edge >= 5
                {
                    // TODO(jon): Can we take a better guess at the approx_head_width using the face mask info?

                    // Take an area of the shape to search within for a neck: the narrowest part, taking
                    // into account some skewing factor
                    info!(
                        "#{}, looking for neck with approx head width {}",
                        get_frame_num(),
                        approx_head_width
                    );
                    let neck = get_neck(&body_shape, approx_head_width);
                    info!("#{}: Neck {:?}", get_frame_num(), neck);
                    // Probably only when there is very low motion in the scene, since that implies we've lost our good head lock.
                    // Also only when the previous head was fully inside the frame.
                    // Refine the threshold data above the neck:

                    // We get back two convex hulls, and some face dimensional info.
                    // We could also just return a raw temperature value and coordinates of the sample point,
                    // that could be good.
                    let face_info = refine_head_threshold_data(
                        neck,
                        point_cloud,
                        median_smoothed.as_ref(),
                        radial_smoothed.as_ref(),
                        thermal_ref_rect,
                    );

                    let thermal_ref_is_on_left = thermal_ref_rect.x1 < WIDTH / 2;
                    let bottom_left = face_info.head.bottom_left;
                    let bottom_right = face_info.head.bottom_right;
                    let neck_is_too_close_to_edge_of_frame = (!thermal_ref_is_on_left
                        && bottom_right.x > (thermal_ref_rect.x0 - 3) as f32)
                        || (thermal_ref_is_on_left
                            && bottom_left.x < (thermal_ref_rect.x1 + 3) as f32)
                        || neck.start.y == 0.0
                        || neck.end.y == 0.0;

                    // TODO(jon): Maybe adjust the amount of head area up a little?
                    if face_info.head.area() > 300.0 {
                        //info!("#{} area: {}", get_frame_num(), face_info.head.area());
                        analysis_result.face = face_info;
                    } else {
                        info!("#{} Head too small?", get_frame_num());
                    }

                    if neck_is_too_close_to_edge_of_frame {
                        info!("#{} neck too close to edge", get_frame_num());
                        // Output the body shape without the head isolated
                        clear_body_shape();
                        BODY_SHAPE.with(|arr_ref| {
                            let mut body_outline = arr_ref.borrow_mut();
                            for span in &body_shape.inner {
                                body_outline.push(span.y);
                                body_outline.push(span.x0);
                                body_outline.push(span.x1);
                            }
                        });
                    }
                    BODY_AREA_THIS_FRAME.with(|a| a.set(body_shape.area()));
                } else {
                    extend_shape_to_bottom(&mut body_shape, 0);

                    BODY_AREA_THIS_FRAME.with(|a| a.set(body_shape.area()));
                    // Output the body shape without the head.
                    clear_body_shape();
                    BODY_SHAPE.with(|arr_ref| {
                        let mut body_outline = arr_ref.borrow_mut();
                        for span in &body_shape.inner {
                            body_outline.push(span.y);
                            body_outline.push(span.x0);
                            body_outline.push(span.x1);
                        }
                    });
                }

                // extend_shape_to_bottom(&mut body_shape, 0);
                //
                // BODY_AREA_THIS_FRAME.with(|a| a.set(body_shape.area()));
                // // Output the body shape without the head.
                // clear_body_shape();
                // BODY_SHAPE.with(|arr_ref| {
                //     let mut body_outline = arr_ref.borrow_mut();
                //     for span in &body_shape.inner {
                //         body_outline.push(span.y);
                //         body_outline.push(span.x0);
                //         body_outline.push(span.x1);
                //     }
                // });
            }
        }
    }
    analysis_result
}

#[allow(unused)]
fn rects_are_vertically_adjacent(a: Rect, b: Rect) -> bool {
    let (top, bottom) = if a.y0 < b.y0 { (a, b) } else { (b, a) };
    // Make sure the shapes don't overlap in y
    if top.y1 < bottom.y0 {
        top.bottom_right().distance_to(bottom.top_right()) < 7.0
            || top.bottom_left().distance_to(bottom.top_left()) < 7.0
    } else {
        false
    }
}

fn merge_shapes(mut largest_shape: SolidShape, mut shapes: Vec<SolidShape>) -> SolidShape {
    loop {
        let mut did_merge = false;
        //let first_bounds = largest_shape.bounds();
        let mut i = 0;
        while i < shapes.len() {
            if let Some(shape) = shapes.pop() {
                //let shape_bounds = shape.bounds();
                //if rects_are_vertically_adjacent(first_bounds, shape_bounds) {
                largest_shape.merge_with(shape);
                did_merge = true;
                break;
                // } else {
                //     shapes.insert(0, shape);
                // }
            }
            i += 1;
        }
        if !did_merge {
            break;
        }
    }

    largest_shape
}

#[wasm_bindgen]
pub fn analyse(
    input_frame: &Uint16Array,
    calibrated_thermal_ref_temp_c: &JsValue,
    ms_since_last_ffc: &JsValue,
) -> AnalysisResult {
    FRAME_NUM.with(|frame_num_ref| {
        let num = frame_num_ref.get();
        frame_num_ref.set(num + 1);
    });

    let ms_since_last_ffc = ms_since_last_ffc.as_f64().unwrap() as u32;

    IMAGE_BUFFERS.with(|buffer_ctx| {
        let calibrated_thermal_ref_temp_c = calibrated_thermal_ref_temp_c.as_f64().unwrap() as f32;

        {
            let mut median_smoothed = buffer_ctx.median_smoothed.borrow_mut();
            // Copy input frame into median_smoothed buffer, for further processing.
            for (dst, src) in median_smoothed
                .pixels_mut()
                .zip(input_frame.to_vec().iter())
            {
                *dst = *src as f32;
            }
        }

        let (motion_shapes, motion_for_current_frame) =
            smooth_internal(buffer_ctx, ms_since_last_ffc);
        let thermal_ref = THERMAL_REF.with(|t_ref| {
            let _p = Perf::new("Detect ref");
            let prev_ref = t_ref.take();

            //buffer_ctx.debug.borrow_mut().buf_mut().copy_from_slice(buffer_ctx.scratch.borrow().buf());

            let thermal_ref = detect_thermal_ref(prev_ref, buffer_ctx);
            t_ref.set(thermal_ref);

            let median_smoothed = buffer_ctx.median_smoothed.borrow();
            if let Some(thermal_ref) = thermal_ref {
                let thermal_ref_raw =
                    extract_sensor_value_for_circle(thermal_ref, median_smoothed.as_ref()).median;
                Some((thermal_ref_raw, thermal_ref.clone()))
            } else {
                //info!("#{} no thermal ref found, prev {:?}", get_frame_num(), prev_ref);
                None
            }
        });

        let mut face: Option<FaceInfo> = None;

        let thermal_ref_rect: Option<Rect> = thermal_ref.map(|(_, thermal_ref)| {
            get_extended_thermal_ref_rect_full_clip(thermal_ref.bounds(), 120, 160)
        });

        let mut analysis_result = if let Some((thermal_ref_raw, thermal_ref)) = thermal_ref {
            let mut analysis_result = extract_internal(
                motion_shapes,
                buffer_ctx,
                thermal_ref_rect.unwrap(),
                motion_for_current_frame,
            );
            analysis_result.thermal_ref = ThermalReference {
                geom: thermal_ref,
                temp: calibrated_thermal_ref_temp_c,
                val: thermal_ref_raw as u16,
            };
            analysis_result.face.sample_temp = temperature_c_for_raw_val(
                calibrated_thermal_ref_temp_c,
                analysis_result.face.sample_value,
                thermal_ref_raw,
            );
            analysis_result.face.ideal_sample_temp = temperature_c_for_raw_val(
                calibrated_thermal_ref_temp_c,
                analysis_result.face.ideal_sample_value,
                thermal_ref_raw,
            );

            // Did we get a real face?
            if analysis_result.face.head.top_left != Point::new(0, 0) {
                face = Some(analysis_result.face.clone());
            }
            analysis_result
        } else {
            let mut analysis_result = AnalysisResult::default();

            let radial_smoothed = buffer_ctx.radial_smoothed.borrow();
            {
                let _p = Perf::new("Global min/max");
                let (min, max) = radial_smoothed
                    .pixels()
                    .fold((f32::MAX, 0.0), |(min, max), val| {
                        (f32::min(val, min), f32::max(val, max))
                    });
                analysis_result.heat_stats.min = min as u16;
                analysis_result.heat_stats.max = max as u16;
            }

            // If no thermal ref, clear body shape:
            clear_body_shape();
            analysis_result
        };

        let prev_face = FACE.with(|face_ref| face_ref.get());
        let prev_frame_has_body = HAS_BODY.with(|body| body.get());

        let prev_state = get_current_state();
        let detected_body = analysis_result.has_body;
        let too_close_to_ffc_event = ms_since_last_ffc < 5000;
        if prev_state.state == ScreeningState::Ready {
            // Require a fair bit of activation motion to consider that we have a body, when transitioning
            // from the ready state.
            if analysis_result.motion_sum < 1000 {
                analysis_result.has_body = false;
                face = None;
            }
        }
        if too_close_to_ffc_event {
            face = None;
        }

        advance_state(
            face,
            prev_face,
            thermal_ref_rect,
            analysis_result.has_body,
            prev_frame_has_body,
            analysis_result.motion_sum,
            too_close_to_ffc_event,
        );

        let next_state = get_current_state();
        if next_state.state == ScreeningState::Ready {
            // If we've transitioned to ready, we don't want to use old motion
            MOTION_BUFFER.with(|buffer| {
                let mut buffer = buffer.borrow_mut();
                buffer.clear();
            });
        }
        if next_state.state == ScreeningState::Ready {
            // Only clear the shape if the shape is floating in the top half of the frame:
            let last_y = BODY_SHAPE.with(|arr_ref| {
                let hull = arr_ref.borrow();
                if hull.len() >= 3 {
                    Some(hull[hull.len() - 3])
                } else {
                    None
                }
            });
            if let Some(last_y) = last_y {
                if (last_y as usize) < HEIGHT - 10 {
                    clear_body_shape();
                    clear_face_shape();
                }
            }
        }
        if !detected_body {
            clear_body_shape();
            clear_face_shape();
        }
        analysis_result.next_state = next_state.state;
        HAS_BODY.with(|body| body.set(analysis_result.has_body));
        FACE.with(|face_ref| face_ref.set(face));

        analysis_result
    })
}

fn subtract_frame(
    prev_radial_smoothed: Img<&[f32]>,
    curr_radial_smoothed: Img<&[f32]>,
    mask: &mut [u8],
    ms_since_last_ffc: u32,
) -> (VecDeque<RawShape>, usize) {
    let _p = Perf::new("Accumulate motion");
    let immediately_after_ffc_event = ms_since_last_ffc < 1000;
    const THRESHOLD_DIFF: &f32 = &40f32; // TODO(jon): This may need tweaking
    let mut motion_for_current_frame = 0;
    let mut motion_shapes = VecDeque::new();
    let is_first_frame_received = prev_radial_smoothed[(0usize, 0usize)] == 0.0;

    let thermal_ref_rect: Rect = THERMAL_REF.with(|thermal_ref| {
        thermal_ref
            .get()
            .map_or(Rect::new(0, 0, 0, 0), |thermal_ref| {
                get_extended_thermal_ref_rect_full_clip(thermal_ref.bounds(), 120, 160)
            })
    });

    // Clear the mask:
    for px in mask.iter_mut() {
        *px = 0;
    }

    let five_seconds_passed_without_motion = false;
    // If it's the first frame received, lets initialise the "min buffer"
    if is_first_frame_received || five_seconds_passed_without_motion {
        IMAGE_BUFFERS.with(|buffers| {
            let mut min_buffer = buffers.min_accumulator.borrow_mut();
            let mut min_buffer = min_buffer.as_mut();
            min_buffer
                .buf_mut()
                .copy_from_slice(curr_radial_smoothed.buf());
        });
    } else {
        // IMAGE_BUFFERS.with(|buffers| {
        //     let mut min_buffer = buffers.min_accumulator.borrow_mut();
        //     let mut min_buffer = min_buffer.as_mut();
        //     for (dest, src) in min_buffer.pixels_mut().zip(curr_radial_smoothed.pixels()) {
        //         if src < *dest {
        //             *dest = *dest + ((src - *dest) * 0.005);
        //         }
        //     }
        // });
    }

    // NOTE: If it's the very first frame, don't accumulate motion since it will be all motion.
    if !is_first_frame_received {
        MOTION_BUFFER.with(|buffer| {
            let mut buffer = buffer.borrow_mut();
            if !immediately_after_ffc_event {
                let _p = Perf::new("Motion for current frame");
                // Accumulate the current frames motion
                buffer.advance();
                for (index, (val_a, (val_b, dest))) in prev_radial_smoothed
                    .pixels()
                    .zip(
                        curr_radial_smoothed
                            .pixels()
                            .zip(buffer.front_mut().next().unwrap()),
                    )
                    .enumerate()
                {
                    // Crop out thermal reference slice:
                    let x = index % WIDTH;
                    let is_in_thermal_ref = x >= thermal_ref_rect.x0 && x < thermal_ref_rect.x1;
                    if !is_in_thermal_ref {
                        *dest = match f32::abs(val_b - val_a).partial_cmp(THRESHOLD_DIFF) {
                            Some(Ordering::Greater) => {
                                // Don't get motion from the top row, it's probably noise
                                if index > 120 {
                                    motion_for_current_frame += 1;
                                    0u8 //MOTION_BIT
                                } else {
                                    0u8
                                }
                            }
                            _ => 0u8,
                        };
                    } else {
                        *dest = 0u8;
                    }
                }
            }

            // NOTE(jon): If there was a large thermal body in the previous frame, and they
            //  haven't moved (no motion), then we'd like to keep using the existing motion hull.
            // Accumulate as many buffers as needed into mask to get our motion high enough.
            // Only accumulate previous frames motion if the current state is not ready.
            buffer.accumulate_into_slice(mask);

            {
                let _p = Perf::new("Subtract min buffer");
                IMAGE_BUFFERS.with(|buffers| {
                    let min_buffer = buffers.min_accumulator.borrow();
                    let min_buffer = min_buffer.as_ref();
                    for (index, ((dest, min), src)) in mask
                        .iter_mut()
                        .zip(min_buffer.pixels())
                        .zip(curr_radial_smoothed.pixels())
                        .enumerate()
                    {
                        const LOWER_BOUND: f32 = 30.0;
                        const UPPER_BOUND: f32 = 50.0;
                        let x = index % WIDTH;
                        let is_in_thermal_ref = x >= thermal_ref_rect.x0 && x < thermal_ref_rect.x1;
                        if !is_in_thermal_ref
                            && (min - src > LOWER_BOUND || src - min > UPPER_BOUND)
                        {
                            *dest |= BACKGROUND_BIT;
                            *dest |= MOTION_BIT;
                        }
                    }
                });
            }

            {
                let _p = Perf::new("Get motion shapes");

                // TODO(jon): Discard shapes that are too small, or have no wide dynamic range.

                motion_shapes = get_raw_shapes(mask, MOTION_BIT);

                // Remove any small motion shapes that are at the top of the frame:
                let num_shapes = motion_shapes.len();
                for _ in 0..num_shapes {
                    if let Some(shape) = motion_shapes.pop_front() {
                        let bounds = shape.bounds();
                        if bounds.y1 > 20 && bounds.y0 != 0 && shape.area() > 5 {
                            motion_shapes.push_back(shape);
                        }
                    }
                }
            }
        });
    }

    (motion_shapes, motion_for_current_frame)
}

fn edge_detect(source: &[f32], dest: &mut [f32], width: isize, height: isize) {
    for y in 2..height - 2 {
        let magic_number = 20.0;
        for x in 2..width - 2 {
            let index = (y * width + x) as usize;
            let a = unsafe { *source.get_unchecked(index) } * 4.0;
            let b = unsafe { *source.get_unchecked(index - 1) };
            let c = unsafe { *source.get_unchecked(index + 1) };
            let d = unsafe { *source.get_unchecked(index + width as usize) };
            let e = unsafe { *source.get_unchecked(index - width as usize) };
            unsafe {
                *dest.get_unchecked_mut(index) = f32::max(a - b - c - d - e - magic_number, 0.0)
            };
        }
    }
}

fn smooth_internal(
    image_buffers: &ImageBuffers,
    ms_since_last_ffc: u32,
) -> (VecDeque<RawShape>, usize) {
    let _p = Perf::new("Smooth internals");
    let median_smoothed = &mut image_buffers.median_smoothed.borrow_mut();
    let radial_smoothed = &mut image_buffers.radial_smoothed.borrow_mut();
    let scratch = &mut image_buffers.scratch.borrow_mut();
    let mask = &mut image_buffers.mask.borrow_mut();
    let edges = &mut image_buffers.edges.borrow_mut();

    // Take a copy of the previous frames radial smoothed output.
    // Should we also have another scratch buffer for this?
    let prev_radial_smoothed = radial_smoothed.clone();

    // Median smooth pass happens before the image is rotated, so width and height are switched.
    median_smooth_pass(median_smoothed.buf_mut(), 1, 0, HEIGHT, WIDTH);
    median_smooth_pass(median_smoothed.buf_mut(), HEIGHT, 0, HEIGHT, WIDTH);
    median_smooth_pass(median_smoothed.buf_mut(), HEIGHT, 3, HEIGHT, WIDTH);
    median_smooth_pass(median_smoothed.buf_mut(), 1, 3, HEIGHT, WIDTH);

    let median_smoothed = median_smoothed;
    radial_smooth_half(median_smoothed.buf(), radial_smoothed.buf_mut(), HEIGHT);
    rotate(median_smoothed.buf(), scratch.buf_mut(), HEIGHT, WIDTH);
    median_smoothed.buf_mut().copy_from_slice(scratch.buf());
    rotate(radial_smoothed.buf(), scratch.buf_mut(), HEIGHT, WIDTH);
    radial_smooth_half(scratch.buf(), radial_smoothed.buf_mut(), WIDTH);

    let (motion_shapes, motion_for_current_frame) = subtract_frame(
        prev_radial_smoothed.as_ref(),
        radial_smoothed.as_ref(),
        mask.buf_mut(),
        ms_since_last_ffc,
    );

    {
        let _p = Perf::new("Isolate thermal ref shape");
        // Threshold the thermal ref, and get eliminate shapes that probably aren't the thermal ref.
        // Then we will do edge detection on what is left, and feed it into the thermal-ref circle
        // detector, though that is probably pretty redundant now.

        // First let's threshold radial_smoothed, taking just the warmest pixels.
        // Take histogram of radial smoothed:
        const NUM_BUCKETS: usize = 16;
        let mut histogram: [u16; NUM_BUCKETS] = [0u16; NUM_BUCKETS];
        let mut min = f32::MAX;
        let mut max = 0.0;
        for val in radial_smoothed
            .sub_image(0, 75, THERMAL_REF_WIDTH, 85)
            .pixels()
            .chain(
                radial_smoothed
                    .sub_image(WIDTH - THERMAL_REF_WIDTH, 75, THERMAL_REF_WIDTH, 85)
                    .pixels(),
            )
        {
            min = f32::min(val, min);
            max = f32::max(val, max);
        }
        let range = max - min;
        for val in radial_smoothed
            .sub_image(0, 75, THERMAL_REF_WIDTH, 85)
            .pixels()
            .chain(
                radial_smoothed
                    .sub_image(WIDTH - THERMAL_REF_WIDTH, 75, THERMAL_REF_WIDTH, 85)
                    .pixels(),
            )
        {
            let bucket_index = usize::min(
                NUM_BUCKETS - 1,
                f32::floor(((val - min) / range) * (NUM_BUCKETS - 1) as f32) as usize,
            );
            histogram[bucket_index] += 1;
        }
        let mut target = 0;
        let mut cut_off_index = histogram.len() - 1;
        for (bucket_index, &val) in histogram.iter().enumerate().rev() {
            if target + val > 250 {
                cut_off_index = bucket_index + 1;
                break;
            } else {
                target += val;
            }
        }
        let threshold = (min as f32) + (range / histogram.len() as f32) * cut_off_index as f32;
        for px in scratch.pixels_mut() {
            *px = 0.0;
        }
        for (dest, src) in scratch
            .sub_image_mut(0, 75, THERMAL_REF_WIDTH, 85)
            .pixels_mut()
            .zip(
                radial_smoothed
                    .sub_image(0, 75, THERMAL_REF_WIDTH, 85)
                    .pixels(),
            )
        {
            if src > threshold {
                *dest = src;
            }
        }
        for (dest, src) in scratch
            .sub_image_mut(WIDTH - THERMAL_REF_WIDTH, 75, THERMAL_REF_WIDTH, 85)
            .pixels_mut()
            .zip(
                radial_smoothed
                    .sub_image(WIDTH - THERMAL_REF_WIDTH, 75, THERMAL_REF_WIDTH, 85)
                    .pixels(),
            )
        {
            if src > threshold {
                *dest = src;
            }
        }

        let mut shapes = get_raw_shapes_over_threshold(scratch.buf(), threshold);
        let num_shapes = shapes.len();
        for _ in 0..num_shapes {
            if let Some(shape) = shapes.pop_front() {
                let center = shape.centroid();
                let bounds = shape.bounds();
                let bounds_center = bounds.centroid();

                let width = bounds.width() as isize;
                let height = bounds.height() as isize;
                let area = shape.area();
                let filled_area_ratio =
                    ((width * height) as f32 - area as f32) / (width * height) as f32;

                let width_height_diff = isize::abs(width - height);
                let center_offset = center.distance_to(bounds_center);
                if width_height_diff <= 3
                    && center_offset < 1.2
                    && area < 250
                    && area > 30
                    && filled_area_ratio <= 0.3
                {
                    shapes.push_back(shape);
                } else {
                    // Erase shape:
                    for spans in shape.inner {
                        if let Some(spans) = spans {
                            for span in spans {
                                for x in span.x0..span.x1 {
                                    scratch[(x as usize, span.y as usize)] = 0.0;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    edge_detect(
        scratch.buf(),
        edges.buf_mut(),
        WIDTH as isize,
        HEIGHT as isize,
    );
    (motion_shapes, motion_for_current_frame)
}

fn extend_shape_to_bottom(shape: &mut SolidShape, start_span_index: usize) {
    if shape.len() != 0 {
        let mut prev_span = shape.inner[usize::min(shape.len() - 1, start_span_index)];
        for i in usize::min(start_span_index + 1, shape.len() - 1)..shape.len() {
            let mut span = &mut shape.inner[i];
            span.x1 = u8::max(span.x1, prev_span.x1);
            span.x0 = u8::min(span.x0, prev_span.x0);
            prev_span = span.clone();
        }
        while prev_span.y < HEIGHT as u8 - 1 {
            let mut dup_span = prev_span.clone();
            dup_span.y = prev_span.y + 1;
            shape.add_span(dup_span.clone());
            prev_span = dup_span;
        }
    }
}

fn refine_head_threshold_data(
    neck: LineSegment,
    point_cloud: Vec<(f32, f32)>,
    median_smoothed: Img<&[f32]>,
    radial_smoothed: Img<&[f32]>,
    thermal_ref_rect: Rect,
) -> FaceInfo {
    let _p = Perf::new("Face info");
    //info!("Got neck {:?} {} points", neck, point_cloud.len());
    let neck_vec = neck.end - neck.start;
    let extend_amount = neck.start.distance_to(neck.end) * 0.1;
    let down_to_chin = neck_vec.perp().perp().perp().norm().scale(extend_amount);
    let scaled_neck_nec = neck_vec.norm().scale(15.0);
    let p0 = neck.start - scaled_neck_nec;
    let p1 = neck.end + scaled_neck_nec;
    let scaled_perp_neck = neck_vec.perp().norm().scale(100.0);
    // let extended_neck_left = neck.start + down_to_chin;
    // let extended_neck_right = neck.end + down_to_chin;

    // TODO(jon): Since we've extended them down, make sure neck points are in frame bounds.

    let neck_base = neck;
    let left_side_of_head = LineSegment {
        start: p0 + scaled_perp_neck,
        end: p0,
    };
    let right_side_of_head = LineSegment {
        start: p1,
        end: p1 + scaled_perp_neck,
    };
    let mut face_info = FaceInfo::default();
    if neck.start.x != 0.0 && neck.end.x != 0.0 && neck.start.y != 0.0 && neck.end.y != 0.0 {
        let (mut head_points, mut body_points): (Vec<_>, Vec<_>) = point_cloud
            .iter()
            .map(|(x, y)| Point { x: *x, y: *y })
            .partition(|point| point.is_left_of_segment(neck_base));

        head_points.push(neck.start);
        head_points.push(neck.end);
        body_points.push(neck.start);
        body_points.push(neck.end);

        let mut body_multi_hull_parts = Vec::new();
        let mut head_hull = None;
        if head_points.len() > 2 {
            let head = MultiPoint::from_iter(
                head_points // reduce_points
                    .iter()
                    .map(|x| x.as_tuple()),
            )
            .convex_hull();
            body_multi_hull_parts.push(head.clone());
            head_hull = Some(head);
        }
        body_multi_hull_parts.push(
            MultiPoint::from_iter(
                body_points // reduce_points
                    .iter()
                    .map(|x| x.as_tuple()),
            )
            .convex_hull(),
        );
        let mut body_shape = get_solid_shapes_for_hull(&MultiPolygon::from(body_multi_hull_parts));

        let neck_bottom_y = u8::max(neck.start.y as u8, neck.end.y as u8);
        let start_index = body_shape
            .inner
            .iter()
            .enumerate()
            .find(|(_, span)| span.y == neck_bottom_y);
        if let Some((start_index, _)) = start_index {
            extend_shape_to_bottom(&mut body_shape, start_index);
        }

        clear_body_shape();
        BODY_SHAPE.with(|arr_ref| {
            let mut body_outline = arr_ref.borrow_mut();
            for span in &body_shape.inner {
                body_outline.push(span.y);
                body_outline.push(span.x0);
                body_outline.push(span.x1);
            }
        });

        // Get the face info:
        if let Some(head_hull) = head_hull {
            let _halfway_ratio = 1.0;
            // Get the left bounds of the head
            let neck_norm = neck_vec.norm();
            let mut inc = 0.0f32;
            let mut sweep_line;
            loop {
                let probe = neck_norm.scale(inc);
                let start = left_side_of_head.end + probe;
                let end = left_side_of_head.start + probe;
                sweep_line = LineSegment { start, end };
                if head_hull
                    .exterior()
                    .points_iter()
                    .map(|p| Point::new(p.x() as usize, p.y() as usize))
                    .find(|p| p.is_left_of_segment(sweep_line))
                    .is_some()
                {
                    break;
                }
                if inc > 160.0 {
                    info!("Didn't find head left point");
                    break;
                }
                inc += 1.0;
            }
            let left_side = sweep_line;

            // Get the right bounds of the head
            let mut inc = 0.0f32;
            loop {
                let probe = neck_norm.scale(-inc);
                let start = right_side_of_head.end + probe;
                let end = right_side_of_head.start + probe;
                sweep_line = LineSegment { start, end };
                if head_hull
                    .exterior()
                    .points_iter()
                    .map(|p| Point::new(p.x() as usize, p.y() as usize))
                    .find(|p| p.is_left_of_segment(sweep_line))
                    .is_some()
                {
                    break;
                }
                if inc > 160.0 {
                    info!("Didn't find head right point");
                    break;
                }
                inc += 1.0;
            }
            let right_side = sweep_line;
            // Get the top bounds of the head
            let mut inc = 0.0f32;
            let head_vertical_norm = (left_side_of_head.start - left_side_of_head.end).norm();
            loop {
                let probe = head_vertical_norm.scale(-inc);
                let start = left_side.end + probe;
                let end = right_side.start + probe;
                sweep_line = LineSegment { start, end };
                if head_hull
                    .exterior()
                    .points_iter()
                    .map(|p| Point::new(p.x() as usize, p.y() as usize))
                    .find(|p| p.is_left_of_segment(sweep_line))
                    .is_some()
                {
                    break;
                }
                if inc > 160.0 {
                    info!("Didn't find head top point");
                    break;
                }
                inc += 1.0;
            }

            let top_side = sweep_line;
            let head_height = top_side.start.distance_to(left_side.start);
            face_info.head.top_left = left_side.start + left_side.norm().scale(head_height);
            face_info.head.top_right = right_side.end - right_side.norm().scale(head_height);
            face_info.head.bottom_left = left_side.start;
            face_info.head.bottom_right = right_side.end;

            let head_width = face_info
                .head
                .bottom_left
                .distance_to(face_info.head.bottom_right);
            let width_to_height_ratio = head_width / head_height;
            let closest_allowed_to_edge = 3.0;

            let thermal_ref_is_on_left = thermal_ref_rect.x1 < WIDTH / 2;
            let head_hull_aabb = head_hull.exterior().bounding_rect().unwrap();
            let Coordinate {
                x: aabb_left,
                y: aabb_top,
            } = head_hull_aabb.min();
            let Coordinate {
                x: aabb_right,
                y: aabb_bottom,
            } = head_hull_aabb.max();
            let neck_is_invalid = neck.start.y == 0.0 || neck.end.y == 0.0;

            let head_is_far_enough_from_edges = if thermal_ref_is_on_left {
                aabb_left > (thermal_ref_rect.x1 as f32) + closest_allowed_to_edge
                    && aabb_right < (WIDTH as f32) - closest_allowed_to_edge
            } else {
                aabb_left > closest_allowed_to_edge
                    && aabb_right < (thermal_ref_rect.x0 as f32) - closest_allowed_to_edge
            } && aabb_top > 1.0f32
                && aabb_bottom < (HEIGHT as f32) - 1.0f32;
            face_info.is_valid =
                width_to_height_ratio > 0.5 && head_is_far_enough_from_edges && !neck_is_invalid;

            let center_neck = neck.start + neck_vec.scale(0.5);
            let _head_left_scale = center_neck.distance_to(face_info.head.bottom_left) / head_width;
            let _head_right_scale =
                center_neck.distance_to(face_info.head.bottom_right) / head_width;

            let d_y = face_info.head.bottom_right.y - face_info.head.bottom_left.y;
            let d_x = face_info.head.bottom_right.x - face_info.head.bottom_left.x;
            let angle = d_y.atan2(d_x) * 180.0 / PI;
            if f32::abs(angle) > 10.0 {
                face_info.head_lock = HeadLockConfidence::Bad;
                face_info.is_valid = false;
            } else if face_info.is_valid {
                // TODO(jon): Now look at symmetry either side of the center line - this requires us to look at the mask:

                // TODO(jon): Get the hottest spot on the face (if not wearing glasses), and assume that this is the inner canthus.
                //  Make the bottom of the head-top above this.

                // If that is hard to find (say a 3x3 uniform patch), then consider using the radial_smoothed buffer to sample from.
                /*
                // Get the hottest point in the face:
                let head_bounds = face_info.head.aa_bounds();
                let mut hottest_val = 0;
                let mut best_p = Point::new(0, 0);
                for y in (head_bounds.y0 as usize)..(head_bounds.y1 as usize) {
                    for x in (head_bounds.x0 as usize)..(head_bounds.x1 as usize) {
                        let p = Point::new(x, y);
                        if point_is_in_triangle(
                            p,
                            face_info.head.top_left,
                            face_info.head.top_right,
                            face_info.head.bottom_left,
                        ) || point_is_in_triangle(
                            p,
                            face_info.head.top_left,
                            face_info.head.top_right,
                            face_info.head.bottom_right,
                        ) {
                            let val = median_smoothed[(x, y)] as u16;
                            if radial_smoothed[(x, y)] as u16 > hottest_val {
                                hottest_val = val;
                                best_p = p;
                            }
                        }
                    }
                }
                // Find out how far down best_p is in the quad:
                // Work out the rotation of the head, then rotate p inverse to that.
                // What is the angle between bottom_left and top_left?
                let d_y = face_info.head.top_left.y - face_info.head.bottom_left.y;
                let d_x = face_info.head.bottom_left.x - face_info.head.top_left.x;
                let angle = d_y.atan2(d_x);
                if get_frame_num() == 29 {
                    info!(
                        "Head angle {}, best_p {:?}, hottest_val {}",
                        angle, best_p, hottest_val
                    );
                }
                */

                // Get the hotspot:
                let mid_left = face_info.head.bottom_left
                    + (face_info.head.top_left - face_info.head.bottom_left).scale(0.6);
                let mid_right = face_info.head.bottom_right
                    + (face_info.head.top_right - face_info.head.bottom_right).scale(0.6);

                let head_top = Quad {
                    bottom_left: mid_left,
                    bottom_right: mid_right,
                    top_left: face_info.head.top_left,
                    top_right: face_info.head.top_right,
                };
                let head_top_bounds = head_top.aa_bounds();

                let ideal_sample_point = center_neck
                    + (face_info.head.top_left - face_info.head.bottom_left)
                        .norm()
                        .scale(head_height * 0.7);

                face_info.ideal_sample_point = ideal_sample_point;
                face_info.ideal_sample_value =
                    median_smoothed[(ideal_sample_point.x as usize, ideal_sample_point.y as usize)];

                // FIXME(jon): Detect both glasses and the inner canthus, and get a horizontal correction
                // from them.

                // FIXME(jon): Sometimes we see a massive temperature gradient around the area that
                // we want to sample - +- 0.5 degrees or more.  Should we try to find a "flat" area
                // where there isn't that much variance?

                // Get a localised threshold:
                let mut vals = Vec::new();
                let mut best_val = None;
                let mut best_point = Point::new(0, 0);
                let mut best_distance = f32::MAX;
                for y in (head_top_bounds.y0 as usize)..(head_top_bounds.y1 as usize) {
                    for x in (head_top_bounds.x0 as usize)..(head_top_bounds.x1 as usize) {
                        let p = Point::new(x, y);
                        if point_is_in_triangle(
                            p,
                            face_info.head.top_left,
                            face_info.head.top_right,
                            face_info.head.bottom_left,
                        ) || point_is_in_triangle(
                            p,
                            face_info.head.top_left,
                            face_info.head.top_right,
                            face_info.head.bottom_right,
                        ) {
                            vals.push(median_smoothed[(x, y)] as u16);
                        }
                    }
                }
                vals.sort_unstable();

                if vals.len() != 0 {
                    // FIXME(jon): This should get the hottest pixel hotter than the 75th percentile
                    // of the head top, right, that is closest to our ideal bounds.  Should we do further
                    // averaging?  This could still pick something too cold if there is a fringe etc.
                    // Make the local_threshold be based on one of the other thresholds we've previously
                    // calculated.

                    let local_threshold = vals[(vals.len() as f32 * 0.95) as usize] as f32;
                    for y in (head_top_bounds.y0 as usize)..(head_top_bounds.y1 as usize) {
                        for x in (head_top_bounds.x0 as usize)..(head_top_bounds.x1 as usize) {
                            let p = Point::new(x, y);
                            if point_is_in_triangle(
                                p,
                                face_info.head.top_left,
                                face_info.head.top_right,
                                face_info.head.bottom_left,
                            ) || point_is_in_triangle(
                                p,
                                face_info.head.top_left,
                                face_info.head.top_right,
                                face_info.head.bottom_right,
                            ) {
                                let raw_temp = median_smoothed[(x, y)];
                                if raw_temp > local_threshold {
                                    let d = ideal_sample_point.distance_sq_to(p);
                                    if d < best_distance {
                                        best_distance = d;
                                        best_point = p;
                                        best_val = Some(raw_temp);
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(val) = best_val {
                    face_info.sample_point = best_point;
                    face_info.sample_value = val;
                    face_info.head_lock = HeadLockConfidence::Stable;
                }
            }
        }
    }
    // Slice off the bottom half of the head hull, and find the center line where both halves are roughly equal.
    face_info
}

#[allow(unused)]
fn get_frame_num() -> isize {
    FRAME_NUM.with(|fr| fr.get())
}

pub fn refine_threshold_data(
    threshold_shapes: &VecDeque<RawShape>,
) -> (Vec<(f32, f32)>, Polygon<f32>) {
    let _p = Perf::new("Refine threshold data");

    //  We basically want to get the points of the outline of each shape
    let points: Vec<_> = threshold_shapes
        .iter()
        .flat_map(|shape| shape.outline_points())
        .collect();

    // Now get rid of outlier points:
    let mut clusters: Vec<Vec<Point>> = Vec::new();
    for &point in &points {
        let mut found_match = false;
        for cluster in &mut clusters {
            let last_point_in_cluster = cluster.last().unwrap().clone();
            if last_point_in_cluster.distance_to(point) < 2.0 {
                found_match = true;
                cluster.push(point);
                break;
            }
        }
        if !found_match {
            clusters.push(vec![point]);
        }
    }

    // For each cluster, what is the distance to the nearest cluster?
    // Testing each corner of the bounding box.  If they are close enough, we should merge boxes?
    let mut clusters_and_bounds = clusters
        .iter()
        .enumerate()
        .map(|(i, c)| (i, get_bounds_for_points(c), None))
        .collect::<Vec<_>>();

    let mut i = 0;
    while i < clusters_and_bounds.len() {
        let (ii, bounds, _) = clusters_and_bounds[i].clone();
        let closest = clusters_and_bounds
            .iter()
            .filter(|(x, _, _)| *x != ii)
            .fold((f32::MAX, 0), |acc, item| {
                let d = bounds.distance_to(&item.1);
                if d < acc.0 {
                    return (d, item.0);
                }
                return acc;
            });
        clusters_and_bounds[i] = (ii, bounds, Some(closest));
        i += 1;
    }
    // TODO(jon): We're just considering distances to corners, but we should really be doing corners with sides?
    let furthest_distances_for_aabbs = 15.0;

    let filtered_clusters = clusters
        .iter()
        .zip(clusters_and_bounds.iter())
        .filter(|(_, c_b)| c_b.2.unwrap().0 < furthest_distances_for_aabbs) //  && c_b.1.area() > 6
        .flat_map(|(c, _)| c.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let p: MultiPoint<_> = filtered_clusters.clone().into();
    let hull: Polygon<f32> = p.convex_hull();

    // NOTE(jon): The closing point is a duplicate of the first, so we can get rid of it.
    (filtered_clusters, hull)
}

#[inline]
fn get_barycentric_coords_for_point(a: Point, b: Point, c: Point, point: Point) -> (f32, f32, f32) {
    let v0 = b - a;
    let v1 = c - a;
    let v2 = point - a;
    let inv = 1.0 / v0.cross(v1);
    let a = v0.cross(v2) * inv;
    let b = v2.cross(v1) * inv;
    let c = 1.0 - a - b;

    (a, b, c)
}

#[inline]
fn point_is_in_triangle(point: Point, p0: Point, p1: Point, p2: Point) -> bool {
    let coords = get_barycentric_coords_for_point(p0, p1, p2, point);
    // if get_frame_num() == 71 && point.y == 45.0 && point.x > 53.0 && point.x < 99.0 {
    //     info!("{:?}", coords);
    // }
    coords.0 > 0.0 && coords.1 > 0.0 && coords.2 > 0.0
}

#[allow(unused)]
fn point_is_in_convex_polygon(point: Point, convex_hull: &Vec<Vec<f32>>) -> bool {
    let p0 = Point::from_raw(&convex_hull[0]);
    // Now make triangles with each set of two points:
    for pair in convex_hull[1..].windows(2) {
        let p1 = Point::from_raw(&pair[0]);
        let p2 = Point::from_raw(&pair[1]);

        if point_is_in_triangle(point, p0, p1, p2) {
            return true;
        }
    }
    false
}

#[allow(unused)]
fn point_is_in_convex_polygon_3(point: Point, convex_hull: &Vec<Vec<f32>>) -> bool {
    // Now make triangles with each set of two points:
    for pair in convex_hull.windows(2) {
        let p1 = Point::from_raw(&pair[0]);
        let p2 = Point::from_raw(&pair[1]);

        if !point.is_left_of_segment(LineSegment { start: p1, end: p2 }) {
            return true;
        }
    }
    false
}

#[allow(unused)]
fn point_is_in_convex_polygon_2(
    point: Point,
    convex_hull: &Vec<Vec<f32>>,
    centroid: Point,
) -> bool {
    for pair in convex_hull.windows(2) {
        let p1 = Point::from_raw(&pair[0]);
        let p2 = Point::from_raw(&pair[1]);

        if point_is_in_triangle(point, centroid, p1, p2) {
            return true;
        }
    }
    false
}

pub fn distance_sq(a: &[u8], b: &[u8]) -> f64 {
    let dx: f32 = a[0] as f32 - b[0] as f32;
    let dy: f32 = a[1] as f32 - b[1] as f32;
    (dx * dx + dy * dy) as f64
}

fn fill_vertical_cracks(shape: &mut SolidShape) {
    // TODO(jon): Do a separate pass to get rid of single pixel inclusions.
    let crack_search_threshold = 5;
    if shape.len() > 1 {
        {
            // Fill in missing rows in y first
            let y_indexes: Vec<_> = shape.inner.iter().map(|span| span.y).enumerate().collect();
            for s in y_indexes.windows(2) {
                let (i, mut y0) = s[0];
                let (_, y1) = s[1];
                while y0 + 1 != y1 {
                    y0 += 1;
                    // Insert a new span which is a clone of the previous row
                    let mut span_to_clone = shape.inner[i].clone();
                    span_to_clone.y = y0;
                    shape.add_span(span_to_clone);
                }
            }
            shape.resort();
        }

        {
            let mut x0_breaks = Vec::new();
            let mut x1_breaks = Vec::new();
            // Look for discontinuities in x0 and x1 of spans.
            for pair in shape.inner.windows(2) {
                let a = pair[0];
                let b = pair[1];
                if (a.x0 as i8 - b.x0 as i8) < -1 {
                    // Seems like a break, find the next row that has an x0 within 5 of us, and join to that.
                    x0_breaks.push(a.y);
                }
                if (a.x1 as i8 - b.x1 as i8) > 1 {
                    x1_breaks.push(a.y);
                }
            }

            let start_y = shape.inner[0].y as usize;
            for b in x0_breaks.iter().rev() {
                let break_start_index = *b as usize - start_y;
                let break_start = shape.inner[break_start_index];
                let break_end = shape
                    .inner
                    .iter()
                    .enumerate()
                    .skip(break_start_index + 1)
                    .find(|(other_index, other)| {
                        let y_offset = other_index - break_start_index;
                        other.x0 <= break_start.x0 + f32::round(y_offset as f32 / 3.0) as u8
                    });

                if let Some((end_index, break_end)) = break_end {
                    let break_end = break_end.clone();
                    let diff = break_end.x0 as i8 - break_start.x0 as i8;
                    let diff_y = ((break_end.y - break_start.y) - 1) as f32;
                    let diff_x = diff as f32;
                    let slope = if diff != 0 { diff_x / diff_y } else { 0.0 };
                    let slope = if slope.is_infinite() { 0.0 } else { slope };
                    let slope = if diff_y == 1.0 { diff as f32 } else { slope };
                    for (i, span_index) in ((break_start_index + 1)..end_index).enumerate() {
                        shape.inner[span_index].x0 =
                            (break_start.x0 as i8 + ((slope * (i + 1) as f32) as i8)) as u8;
                    }
                }
                //Find the break end, then fill the gap.
            }
            for b in x1_breaks.iter().rev() {
                let break_start_index = *b as usize - start_y;
                let break_start = shape.inner[break_start_index];
                let break_end = shape
                    .inner
                    .iter()
                    .enumerate()
                    .skip(break_start_index + 1)
                    .find(|(other_index, other)| {
                        let y_offset = other_index - break_start_index;
                        other.x1 >= break_start.x1 - f32::round(y_offset as f32 / 3.0) as u8
                    });
                if let Some((end_index, break_end)) = break_end {
                    let break_end = break_end.clone();
                    let diff = break_end.x1 as i8 - break_start.x1 as i8;

                    let diff_y = ((break_end.y - break_start.y) - 1) as f32;
                    let diff_x = diff as f32;
                    let slope = if diff != 0 { diff_x / diff_y } else { 0.0 };
                    let slope = if slope.is_infinite() { 0.0 } else { slope };
                    let slope = if diff_y == 0.0 { diff as f32 } else { slope };

                    for (i, span_index) in ((break_start_index + 1)..end_index).enumerate() {
                        shape.inner[span_index].x1 =
                            (break_start.x1 as i8 + ((slope * (i + 1) as f32) as i8)) as u8;
                    }
                }
            }
        }
    }
}

fn draw_raw_shapes_into_mask(
    shapes: &VecDeque<RawShape>,
    mask: &mut [u8],
    bit: u8,
    min_shape_area: u16,
) {
    for shape in shapes.iter().filter(|shape| shape.area() > min_shape_area) {
        for row in shape.inner.iter() {
            if let Some(row) = row {
                for span in row {
                    for x in span.x0..span.x1 {
                        unsafe {
                            *mask.get_unchecked_mut(span.y as usize * WIDTH + x as usize) |= bit
                        };
                    }
                }
            }
        }
    }
}

fn get_bounds_for_points(points: &Vec<Point>) -> Rect {
    let x0 = points
        .iter()
        .fold(f32::MAX, |acc, point| f32::min(point.x, acc)) as usize;
    let x1 = points.iter().fold(0.0, |acc, point| f32::max(point.x, acc)) as usize + 1;
    let y0 = points
        .iter()
        .fold(f32::MAX, |acc, point| f32::min(point.y, acc)) as usize;
    let y1 = points.iter().fold(0.0, |acc, point| f32::max(point.y, acc)) as usize;
    return Rect { x0, x1, y0, y1 };
}

fn get_solid_shapes_from_hull_2(
    hull: &Polygon<f32>,
    bounds: &GeoRect<f32>,
    threshold_shapes: &VecDeque<RawShape>,
) -> Vec<SolidShape> {
    let _p = Perf::new("Get threshold raw shapes");
    let mut shapes = RawShape::new();
    let Coordinate { x: x0, y: y0 } = bounds.min();
    let Coordinate { x: x1, y: y1 } = bounds.max();
    let x0 = x0 as u8;
    let x1 = x1 as u8;
    let y0 = y0 as u8;
    let y1 = y1 as u8;

    for shape in threshold_shapes {
        // We basically want to add all the shapes into one shape, and fill horizontal gaps between them.
        for row in shape.inner.iter().filter_map(|x| x.as_ref()) {
            for &span in row.iter().filter(|&span| span.y >= y0 && span.y < y1) {
                // Clip span to bounds of hull
                let span = span.clone();
                let y = span.y;
                let mut x_start = u8::min(u8::max(span.x0, x0), x1);
                let mut x_end = u8::max(u8::min(span.x1, x1), x0);
                while x_start < x_end
                    && !hull.contains(&Coordinate::from((x_start as f32, y as f32)))
                {
                    x_start += 1;
                }
                while x_end > x_start && !hull.contains(&Coordinate::from((x_end as f32, y as f32)))
                {
                    x_end -= 1;
                }
                shapes.add_or_extend_span(Span {
                    x0: x_start,
                    x1: u8::max(x_start + 1, x_end),
                    y: span.y,
                });
            }
        }
    }

    let mut solid_shapes: Vec<SolidShape> = Vec::new();
    solid_shapes.push(SolidShape::new());
    // Now convert the raw shape into a bunch of solid shapes:
    for span in shapes
        .inner
        .iter()
        .filter_map(|x| x.as_ref())
        .map(|x| x.first())
    {
        match span {
            Some(&span) => {
                solid_shapes.last_mut().unwrap().add_span(span);
            }
            None => {
                solid_shapes.push(SolidShape::new());
            }
        }
    }
    solid_shapes
}

fn get_raw_shapes(mask: &[u8], bit: u8) -> VecDeque<RawShape> {
    let mut shapes = VecDeque::new();
    for y in 0..HEIGHT {
        let mut span = None;
        for x in 0..WIDTH {
            let index = y * WIDTH + x;
            if mask[index] & bit != 0 {
                if span.is_none() {
                    let mut new_span = Span::new();
                    new_span.x0 = x as u8;
                    new_span.y = y as u8;
                    span = Some(new_span);
                }
            } else {
                if let Some(mut span) = span.take() {
                    // Close the span
                    span.x1 = x as u8;
                    span.assign_to_shape(&mut shapes);
                }
            }
        }
        if let Some(mut span) = span.take() {
            // Close span if we got to the end of the line without closing.
            if span.x1 == 0 {
                span.x1 = WIDTH as u8;
                span.assign_to_shape(&mut shapes);
            }
        }
    }
    shapes
}

fn get_raw_shapes_over_threshold(mask: &[f32], threshold: f32) -> VecDeque<RawShape> {
    let mut shapes = VecDeque::new();
    for y in 0..HEIGHT {
        let mut span = None;
        for x in 0..WIDTH {
            let index = y * WIDTH + x;
            if mask[index] > threshold {
                if span.is_none() {
                    let mut new_span = Span::new();
                    new_span.x0 = x as u8;
                    new_span.y = y as u8;
                    span = Some(new_span);
                }
            } else {
                if let Some(mut span) = span.take() {
                    // Close the span
                    span.x1 = x as u8;
                    span.assign_to_shape(&mut shapes);
                }
            }
        }
        if let Some(mut span) = span.take() {
            // Close span if we got to the end of the line without closing.
            if span.x1 == 0 {
                span.x1 = WIDTH as u8;
                span.assign_to_shape(&mut shapes);
            }
        }
    }
    shapes
}

#[wasm_bindgen(js_name=getMedianSmoothed)]
pub fn get_median_smoothed() -> Float32Array {
    IMAGE_BUFFERS.with(|image_buffers| {
        let median_smoothed = image_buffers.median_smoothed.borrow();
        unsafe { Float32Array::view(median_smoothed.buf()) }
    })
}

#[wasm_bindgen(js_name=getDebug)]
pub fn get_debug() -> Float32Array {
    IMAGE_BUFFERS.with(|image_buffers| {
        let debug = image_buffers.debug.borrow();
        unsafe { Float32Array::view(debug.buf()) }
    })
}

#[wasm_bindgen(js_name=getThresholded)]
pub fn get_thresholded() -> Uint8Array {
    IMAGE_BUFFERS.with(|image_buffers| {
        let mask = image_buffers.mask.borrow();
        unsafe { Uint8Array::view(mask.buf()) }
    })
}

#[wasm_bindgen(js_name=getBodyShape)]
pub fn get_body_shape() -> Uint8Array {
    BODY_SHAPE.with(|body_shape| {
        let body_shape = body_shape.borrow();
        unsafe { Uint8Array::view(&body_shape) }
    })
}

#[wasm_bindgen(js_name=getFaceShape)]
pub fn get_face_shape() -> Uint8Array {
    FACE_SHAPE.with(|face_shape| {
        let face_shape = face_shape.borrow();
        unsafe { Uint8Array::view(&face_shape) }
    })
}

#[wasm_bindgen(js_name=getRadialSmoothed)]
pub fn get_radial_smoothed() -> Float32Array {
    IMAGE_BUFFERS.with(|image_buffers| {
        let radial_smoothed = image_buffers.radial_smoothed.borrow();
        unsafe { Float32Array::view(radial_smoothed.buf()) }
    })
}

#[wasm_bindgen(js_name=getEdges)]
pub fn get_edges() -> Float32Array {
    IMAGE_BUFFERS.with(|image_buffers| {
        let edges = image_buffers.edges.borrow();
        unsafe { Float32Array::view(edges.buf()) }
    })
}

fn temperature_c_for_raw_val(
    calibrated_thermal_ref_c: f32,
    sample_raw_val: f32,
    current_thermal_ref_raw_val: f32,
) -> f32 {
    calibrated_thermal_ref_c + (sample_raw_val - current_thermal_ref_raw_val) * 0.01
}
