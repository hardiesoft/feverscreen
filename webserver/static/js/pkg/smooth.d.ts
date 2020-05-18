/* tslint:disable */
/* eslint-disable */
/**
* @param {any} width 
* @param {any} height 
*/
export function initialize(width: any, height: any): void;
/**
* @param {any} input_frame 
*/
export function smooth(input_frame: any): void;
/**
* @returns {any} 
*/
export function get_median_smoothed(): any;
/**
* @returns {any} 
*/
export function get_radial_smoothed(): any;
/**
* @returns {any} 
*/
export function get_edges(): any;
/**
* @param {any} input_frame 
* @returns {any} 
*/
export function median_smooth(input_frame: any): any;
/**
* @param {any} input_frame 
* @returns {any} 
*/
export function radial_smooth(input_frame: any): any;

/**
* If `module_or_path` is {RequestInfo}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {RequestInfo | BufferSource | WebAssembly.Module} module_or_path
*
* @returns {Promise<any>}
*/
export default function init (module_or_path?: RequestInfo | BufferSource | WebAssembly.Module): Promise<any>;
        