import { CameraConnectionState, Frame } from "./camera";
import { Face } from "./face";
import { HaarCascade } from "./haar-cascade";
import { ROIFeature } from "./worker-fns";

export type BoxOffset = "left" | "right" | "top" | "bottom";
export interface CropBox {
  left: number;
  right: number;
  top: number;
  bottom: number;
}
export interface BoundingBox {
  x0: number;
  x1: number;
  y0: number;
  y1: number;
}

export interface TestAppState {
  thermalReference: ROIFeature | null;
  faces: Face[];
  cropBox: CropBox;
}

export enum ScreeningState {
  WARMING_UP = "WARMING_UP",
  READY = "READY", // no face
  HEAD_LOCK = "HEAD_LOCK",
  MULTIPLE_HEADS = "MULTIPLE_HEADS",
  FACE_LOCK = "FACE_LOCK", // has face
  FRONTAL_LOCK = "FRONTAL_LOCK", // Face is front-on
  STABLE_LOCK = "STABLE_LOCK", // Face has not changed in size or position for a couple of frames.
  LEAVING = "LEAVING" // has face, but not front-on
}

// This describes the state machine of allowed state transitions for the screening event.
export const ScreeningAcceptanceStates = {
  [ScreeningState.WARMING_UP]: [ScreeningState.READY],
  [ScreeningState.MULTIPLE_HEADS]: [
    ScreeningState.READY,
    ScreeningState.HEAD_LOCK,
    ScreeningState.FACE_LOCK,
    ScreeningState.FRONTAL_LOCK
  ],
  [ScreeningState.READY]: [
    ScreeningState.HEAD_LOCK,
    ScreeningState.MULTIPLE_HEADS,
    ScreeningState.FACE_LOCK,
    ScreeningState.FRONTAL_LOCK
  ],
  [ScreeningState.FACE_LOCK]: [
    ScreeningState.HEAD_LOCK,
    ScreeningState.MULTIPLE_HEADS,
    ScreeningState.FRONTAL_LOCK,
    ScreeningState.READY
  ],
  [ScreeningState.FRONTAL_LOCK]: [
    ScreeningState.STABLE_LOCK,
    ScreeningState.FACE_LOCK,
    ScreeningState.MULTIPLE_HEADS,
    ScreeningState.HEAD_LOCK,
    ScreeningState.READY
  ],
  [ScreeningState.HEAD_LOCK]: [
    ScreeningState.FACE_LOCK,
    ScreeningState.FRONTAL_LOCK,
    ScreeningState.READY,
    ScreeningState.MULTIPLE_HEADS
  ],
  [ScreeningState.STABLE_LOCK]: [ScreeningState.LEAVING],
  [ScreeningState.LEAVING]: [ScreeningState.READY]
};

export interface AppState {
  currentFrame: Frame | null;
  cameraConnectionState: CameraConnectionState;
  thermalReference: ROIFeature | null;
  faces: Face[];
  calibrationTemperature: DegreesCelsius;
  currentScreeningState: ScreeningState;
  paused: boolean;
  faceModel: HaarCascade | null;
  lastFrameTime: number;
  cropBox: CropBox;
}

export class DegreesCelsius {
  public val: number;
  constructor(val: number) {
    this.val = val;
  }
  public toString(): string {
    return `${this.val.toFixed(2)}°C`;
  }
}
