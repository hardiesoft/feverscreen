/******/ (function(modules) { // webpackBootstrap
/******/ 	// The module cache
/******/ 	var installedModules = {};
/******/
/******/ 	// The require function
/******/ 	function __webpack_require__(moduleId) {
/******/
/******/ 		// Check if module is in cache
/******/ 		if(installedModules[moduleId]) {
/******/ 			return installedModules[moduleId].exports;
/******/ 		}
/******/ 		// Create a new module (and put it into the cache)
/******/ 		var module = installedModules[moduleId] = {
/******/ 			i: moduleId,
/******/ 			l: false,
/******/ 			exports: {}
/******/ 		};
/******/
/******/ 		// Execute the module function
/******/ 		modules[moduleId].call(module.exports, module, module.exports, __webpack_require__);
/******/
/******/ 		// Flag the module as loaded
/******/ 		module.l = true;
/******/
/******/ 		// Return the exports of the module
/******/ 		return module.exports;
/******/ 	}
/******/
/******/
/******/ 	// expose the modules object (__webpack_modules__)
/******/ 	__webpack_require__.m = modules;
/******/
/******/ 	// expose the module cache
/******/ 	__webpack_require__.c = installedModules;
/******/
/******/ 	// define getter function for harmony exports
/******/ 	__webpack_require__.d = function(exports, name, getter) {
/******/ 		if(!__webpack_require__.o(exports, name)) {
/******/ 			Object.defineProperty(exports, name, { enumerable: true, get: getter });
/******/ 		}
/******/ 	};
/******/
/******/ 	// define __esModule on exports
/******/ 	__webpack_require__.r = function(exports) {
/******/ 		if(typeof Symbol !== 'undefined' && Symbol.toStringTag) {
/******/ 			Object.defineProperty(exports, Symbol.toStringTag, { value: 'Module' });
/******/ 		}
/******/ 		Object.defineProperty(exports, '__esModule', { value: true });
/******/ 	};
/******/
/******/ 	// create a fake namespace object
/******/ 	// mode & 1: value is a module id, require it
/******/ 	// mode & 2: merge all properties of value into the ns
/******/ 	// mode & 4: return value when already ns object
/******/ 	// mode & 8|1: behave like require
/******/ 	__webpack_require__.t = function(value, mode) {
/******/ 		if(mode & 1) value = __webpack_require__(value);
/******/ 		if(mode & 8) return value;
/******/ 		if((mode & 4) && typeof value === 'object' && value && value.__esModule) return value;
/******/ 		var ns = Object.create(null);
/******/ 		__webpack_require__.r(ns);
/******/ 		Object.defineProperty(ns, 'default', { enumerable: true, value: value });
/******/ 		if(mode & 2 && typeof value != 'string') for(var key in value) __webpack_require__.d(ns, key, function(key) { return value[key]; }.bind(null, key));
/******/ 		return ns;
/******/ 	};
/******/
/******/ 	// getDefaultExport function for compatibility with non-harmony modules
/******/ 	__webpack_require__.n = function(module) {
/******/ 		var getter = module && module.__esModule ?
/******/ 			function getDefault() { return module['default']; } :
/******/ 			function getModuleExports() { return module; };
/******/ 		__webpack_require__.d(getter, 'a', getter);
/******/ 		return getter;
/******/ 	};
/******/
/******/ 	// Object.prototype.hasOwnProperty.call
/******/ 	__webpack_require__.o = function(object, property) { return Object.prototype.hasOwnProperty.call(object, property); };
/******/
/******/ 	// __webpack_public_path__
/******/ 	__webpack_require__.p = "/static/dist/";
/******/
/******/
/******/ 	// Load entry module and return exports
/******/ 	return __webpack_require__(__webpack_require__.s = "fbd5");
/******/ })
/************************************************************************/
/******/ ({

/***/ "4847":
/***/ (function(module, exports, __webpack_require__) {

module.exports = function() {
  return new Worker(__webpack_require__.p + "2c4ea705df3edbf4635b.worker.js");
};

/***/ }),

/***/ "fbd5":
/***/ (function(module, __webpack_exports__, __webpack_require__) {

"use strict";
// ESM COMPAT FLAG
__webpack_require__.r(__webpack_exports__);

// EXPORTS
__webpack_require__.d(__webpack_exports__, "processSensorData", function() { return /* binding */ processSensorData; });

// CONCATENATED MODULE: ./src/utils.ts
const BlobReader = function () {
  // For comparability with older browsers/iOS that don't yet support arrayBuffer()
  // directly off the blob object
  const arrayBuffer = "arrayBuffer" in Blob.prototype && typeof Blob.prototype["arrayBuffer"] === "function" ? blob => blob["arrayBuffer"]() : blob => new Promise((resolve, reject) => {
    const fileReader = new FileReader();
    fileReader.addEventListener("load", () => {
      resolve(fileReader.result);
    });
    fileReader.addEventListener("error", () => {
      reject();
    });
    fileReader.readAsArrayBuffer(blob);
  });
  return {
    arrayBuffer
  };
}();
class DegreesCelsius {
  constructor(val) {
    this.val = val;
  }

  toString() {
    if (this.val === undefined) {
      debugger;
    }

    return `${this.val.toFixed(1)}°`;
  }

}
const getHistogram = (data, numBuckets) => {
  // Find find the total range of the data
  let max = 0;
  let min = Number.MAX_SAFE_INTEGER;

  for (let i = 0; i < data.length; i++) {
    const u16Val = data[i];

    if (u16Val < min) {
      min = u16Val;
    }

    if (u16Val > max) {
      max = u16Val;
    }
  } // A histogram with 16 buckets seems sufficiently coarse for this
  // The histogram is usually bi-modal, so we want to find the trough between the two peaks as our threshold value


  const histogram = new Uint16Array(numBuckets);

  for (let i = 0; i < data.length; i++) {
    // If within a few degrees of constant heat-source white else black.
    const v = (data[i] - min) / (max - min) * numBuckets;
    histogram[Math.floor(v)] += 1;
  }

  return {
    histogram,
    min,
    max
  };
};
const getAdaptiveThreshold = data => {
  const {
    histogram,
    min,
    max
  } = getHistogram(data, 16);
  let peak0Max = 0;
  let peak1Max = 0;
  let peak0Index = 0;
  let peak1Index = 0; // First, find the peak value, which is usually in the background temperature range.

  for (let i = 0; i < histogram.length; i++) {
    if (histogram[i] > peak0Max) {
      peak0Max = histogram[i];
      peak0Index = i;
    }
  } // Need to look for a valley in between the two bimodal peaks, so
  // keep descending from the first peak till we find a local minimum.


  let prevIndex = peak0Index;
  let index = peak0Index + 1;

  while (histogram[index] < histogram[prevIndex]) {
    prevIndex++;
    index++;
  } // Now climb up from that valley to find the second peak.


  for (let i = prevIndex; i < histogram.length; i++) {
    if (histogram[i] > peak1Max) {
      peak1Max = histogram[i];
      peak1Index = i;
    }
  } // Find the lowest point between the two peaks.


  let valleyMin = peak1Max;
  let valleyIndex = 0;

  for (let i = peak0Index + 1; i < peak1Index; i++) {
    if (histogram[i] < valleyMin) {
      valleyMin = histogram[i];
      valleyIndex = i;
    }
  }

  const range = max - min;
  return min + range / histogram.length * valleyIndex + 1;
};
const temperatureForSensorValue = (savedThermalRefValue, rawValue, currentThermalRefValue) => {
  return new DegreesCelsius(savedThermalRefValue + (rawValue - currentThermalRefValue) * 0.01);
};
const ZeroCelsiusInKelvin = 273.15;
const mKToCelsius = mkVal => new DegreesCelsius(mkVal * 0.01 - ZeroCelsiusInKelvin);
function saveCurrentVersion(binaryVersion, appVersion) {
  window.localStorage.setItem("softwareVersion", JSON.stringify({
    appVersion,
    binaryVersion
  }));
}
function checkForSoftwareUpdates(binaryVersion, appVersion, shouldReloadIfChanged = true) {
  const prevVersionJSON = window.localStorage.getItem("softwareVersion");

  if (prevVersionJSON) {
    try {
      const prevVersion = JSON.parse(prevVersionJSON);

      if (prevVersion.binaryVersion != binaryVersion || prevVersion.appVersion != appVersion) {
        if (shouldReloadIfChanged) {
          /*
          console.log(
            "reload because version changed",
            JSON.stringify(prevVersion),
            binaryVersion,
            appVersion
          );
          window.location.reload();
                       */
        } else {
          saveCurrentVersion(binaryVersion, appVersion); // Display info that the software has updated since last started up.

          return true;
        }
      }
    } catch (e) {
      saveCurrentVersion(binaryVersion, appVersion);
      return false;
    }
  } else {
    saveCurrentVersion(binaryVersion, appVersion);
  }

  return false;
}
// CONCATENATED MODULE: ./src/camera.ts

var CameraConnectionState;

(function (CameraConnectionState) {
  CameraConnectionState[CameraConnectionState["Connecting"] = 0] = "Connecting";
  CameraConnectionState[CameraConnectionState["Connected"] = 1] = "Connected";
  CameraConnectionState[CameraConnectionState["Disconnected"] = 2] = "Disconnected";
})(CameraConnectionState || (CameraConnectionState = {}));

const UUID = new Date().getTime();
class camera_CameraConnection {
  constructor(host, port, onFrame, onConnectionStateChange) {
    this.host = host;
    this.onFrame = onFrame;
    this.onConnectionStateChange = onConnectionStateChange;
    this.state = {
      socket: null,
      UUID: new Date().getTime(),
      stats: {
        skippedFramesServer: 0,
        skippedFramesClient: 0
      },
      pendingFrame: null,
      prevFrameNum: -1,
      heartbeatInterval: 0,
      frames: []
    };

    if (port === "8080" || port === "5000") {
      // If we're running in development mode, find the remote camera server
      this.host = "192.168.178.21";
    }

    this.connect();
  }

  retryConnection(retryTime) {
    if (retryTime > 0) {
      setTimeout(() => this.retryConnection(retryTime - 1), 1000);
    } else {
      this.connect();
    }
  }

  register() {
    if (this.state.socket !== null) {
      if (this.state.socket.readyState === WebSocket.OPEN) {
        // We are waiting for frames now.
        this.state.socket.send(JSON.stringify({
          type: "Register",
          data: navigator.userAgent,
          uuid: UUID
        }));
        this.onConnectionStateChange(CameraConnectionState.Connected);
        this.state.heartbeatInterval = setInterval(() => {
          this.state.socket && this.state.socket.send(JSON.stringify({
            type: "Heartbeat",
            uuid: UUID
          }));
        }, 5000);
      } else {
        setTimeout(this.register.bind(this), 100);
      }
    }
  }

  connect() {
    var _this = this;

    this.state.socket = new WebSocket(`ws://${this.host}/ws`);
    this.onConnectionStateChange(CameraConnectionState.Connecting);
    this.state.socket.addEventListener("error", e => {
      console.warn("Websocket Connection error", e); //...
    }); // Connection opened

    this.state.socket.addEventListener("open", this.register.bind(this));
    this.state.socket.addEventListener("close", () => {
      // When we do reconnect, we need to treat it as a new connection
      console.warn("Websocket closed");
      this.state.socket = null;
      this.onConnectionStateChange(CameraConnectionState.Disconnected);
      clearInterval(this.state.heartbeatInterval);
      this.retryConnection(5);
    });
    this.state.socket.addEventListener("message", async function (event) {
      if (event.data instanceof Blob) {
        // TODO(jon): Only do this if we detect that we're dropping frames?
        const droppingFrames = false;

        if (droppingFrames) {
          _this.state.frames.push(await _this.parseFrame(event.data)); // Process the latest frame, after waiting half a frame delay
          // to see if there are any more frames hot on its heels.


          _this.state.pendingFrame = setTimeout(_this.useLatestFrame.bind(_this), 1);
        } else {
          _this.onFrame(await _this.parseFrame(event.data));
        } // Every time we get a frame, set a new timeout for when we decide that the camera has stalled sending us new frames.

      }
    });
  }

  async parseFrame(blob) {
    // NOTE(jon): On iOS. it seems slow to do multiple fetches from the blob, so let's do it all at once.
    const data = await BlobReader.arrayBuffer(blob);
    const frameInfoLength = new Uint16Array(data.slice(0, 2))[0];
    const frameStartOffset = 2 + frameInfoLength;

    try {
      const frameInfo = JSON.parse(String.fromCharCode(...new Uint8Array(data.slice(2, frameStartOffset))));
      const frameNumber = frameInfo.Telemetry.FrameCount;

      if (frameNumber % 20 === 0) {
        performance.clearMarks();
        performance.clearMeasures();
        performance.clearResourceTimings();
      }

      performance.mark(`start frame ${frameNumber}`);

      if (this.state.prevFrameNum !== -1 && this.state.prevFrameNum + 1 !== frameInfo.Telemetry.FrameCount) {
        this.state.stats.skippedFramesServer += frameInfo.Telemetry.FrameCount - this.state.prevFrameNum; // Work out an fps counter.
      }

      this.state.prevFrameNum = frameInfo.Telemetry.FrameCount;
      const frameSizeInBytes = frameInfo.Camera.ResX * frameInfo.Camera.ResY * 2; // TODO(jon): Some perf optimisations here.

      const frame = new Uint16Array(data.slice(frameStartOffset, frameStartOffset + frameSizeInBytes));
      return {
        frameInfo,
        frame
      };
    } catch (e) {
      console.error("Malformed JSON payload", e);
    }

    return null;
  }

  async useLatestFrame() {
    if (this.state.pendingFrame) {
      clearTimeout(this.state.pendingFrame);
    }

    let latestFrameTimeOnMs = 0;
    let latestFrame = null; // Turns out that we don't always get the messages in order from the pi, so make sure we take the latest one.

    const framesToDrop = [];

    while (this.state.frames.length !== 0) {
      const frame = this.state.frames.shift();
      const frameHeader = frame.frameInfo;
      const timeOn = frameHeader.Telemetry.TimeOn / 1000 / 1000;

      if (timeOn > latestFrameTimeOnMs) {
        if (latestFrame !== null) {
          framesToDrop.push(latestFrame);
        }

        latestFrameTimeOnMs = timeOn;
        latestFrame = frame;
      }
    } // Clear out and log any old frames that need to be dropped


    while (framesToDrop.length !== 0) {
      const dropFrame = framesToDrop.shift();
      const timeOn = dropFrame.frameInfo.Telemetry.TimeOn / 1000 / 1000;
      this.state.stats.skippedFramesClient++;

      if (this.state.socket) {
        this.state.socket.send(JSON.stringify({
          type: "Dropped late frame",
          data: `${latestFrameTimeOnMs - timeOn}ms behind current: frame#${dropFrame.frameInfo.Telemetry.FrameCount}`,
          uuid: UUID
        }));
      } else {
        console.warn("Lost web socket connection");
      }
    } // Take the latest frame and process it.


    if (latestFrame !== null) {
      await this.onFrame(latestFrame); // if (DEBUG_MODE) {
      //   updateFpsCounter(skippedFramesServer, skippedFramesClient);
      // } else if (fpsCount.innerText !== "") {
      //   fpsCount.innerText = "";
      // }
    }

    this.state.stats.skippedFramesClient = 0;
    this.state.stats.skippedFramesServer = 0;
  }

}
// CONCATENATED MODULE: ./cptv-player/cptv_player.js
(function () {
  const __exports = {};
  let wasm;
  /**
  * @param {number} size
  */

  __exports.initBufferWithSize = function (size) {
    wasm.initBufferWithSize(size);
  };

  let cachegetUint8Memory = null;

  function getUint8Memory() {
    if (cachegetUint8Memory === null || cachegetUint8Memory.buffer !== wasm.memory.buffer) {
      cachegetUint8Memory = new Uint8Array(wasm.memory.buffer);
    }

    return cachegetUint8Memory;
  }

  let WASM_VECTOR_LEN = 0;

  function passArray8ToWasm(arg) {
    const ptr = wasm.__wbindgen_malloc(arg.length * 1);

    getUint8Memory().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
  }
  /**
  * @param {Uint8Array} chunk
  * @param {number} offset
  */


  __exports.insertChunkAtOffset = function (chunk, offset) {
    wasm.insertChunkAtOffset(passArray8ToWasm(chunk), WASM_VECTOR_LEN, offset);
  };

  const heap = new Array(32);
  heap.fill(undefined);
  heap.push(undefined, null, true, false);

  function getObject(idx) {
    return heap[idx];
  }

  let heap_next = heap.length;

  function dropObject(idx) {
    if (idx < 36) return;
    heap[idx] = heap_next;
    heap_next = idx;
  }

  function takeObject(idx) {
    const ret = getObject(idx);
    dropObject(idx);
    return ret;
  }
  /**
  * @param {Uint8Array} input
  * @returns {any}
  */


  __exports.initWithCptvData = function (input) {
    const ret = wasm.initWithCptvData(passArray8ToWasm(input), WASM_VECTOR_LEN);
    return takeObject(ret);
  };
  /**
  * @returns {number}
  */


  __exports.getNumFrames = function () {
    const ret = wasm.getNumFrames();
    return ret >>> 0;
  };
  /**
  * @returns {number}
  */


  __exports.getWidth = function () {
    const ret = wasm.getWidth();
    return ret >>> 0;
  };
  /**
  * @returns {number}
  */


  __exports.getHeight = function () {
    const ret = wasm.getHeight();
    return ret >>> 0;
  };
  /**
  * @returns {number}
  */


  __exports.getFrameRate = function () {
    const ret = wasm.getFrameRate();
    return ret;
  };
  /**
  * @returns {number}
  */


  __exports.getFramesPerIframe = function () {
    const ret = wasm.getFramesPerIframe();
    return ret;
  };
  /**
  * @returns {number}
  */


  __exports.getMinValue = function () {
    const ret = wasm.getMinValue();
    return ret;
  };
  /**
  * @returns {number}
  */


  __exports.getMaxValue = function () {
    const ret = wasm.getMaxValue();
    return ret;
  };
  /**
  * @returns {any}
  */


  __exports.getHeader = function () {
    const ret = wasm.getHeader();
    return takeObject(ret);
  };

  function addHeapObject(obj) {
    if (heap_next === heap.length) heap.push(heap.length + 1);
    const idx = heap_next;
    heap_next = heap[idx];
    heap[idx] = obj;
    return idx;
  }
  /**
  * @param {number} number
  * @param {any} callback
  * @returns {boolean}
  */


  __exports.queueFrame = function (number, callback) {
    const ret = wasm.queueFrame(number, addHeapObject(callback));
    return ret !== 0;
  };
  /**
  * @param {number} number
  * @param {Uint8Array} image_data
  * @returns {boolean}
  */


  __exports.getFrame = function (number, image_data) {
    const ptr0 = passArray8ToWasm(image_data);
    const len0 = WASM_VECTOR_LEN;

    try {
      const ret = wasm.getFrame(number, ptr0, len0);
      return ret !== 0;
    } finally {
      image_data.set(getUint8Memory().subarray(ptr0 / 1, ptr0 / 1 + len0));

      wasm.__wbindgen_free(ptr0, len0 * 1);
    }
  };
  /**
  * @param {Uint8Array} image_data
  * @returns {FrameHeaderV2}
  */


  __exports.getRawFrame = function (image_data) {
    const ptr0 = passArray8ToWasm(image_data);
    const len0 = WASM_VECTOR_LEN;

    try {
      const ret = wasm.getRawFrame(ptr0, len0);
      return FrameHeaderV2.__wrap(ret);
    } finally {
      image_data.set(getUint8Memory().subarray(ptr0 / 1, ptr0 / 1 + len0));

      wasm.__wbindgen_free(ptr0, len0 * 1);
    }
  };

  let cachedTextDecoder = new TextDecoder('utf-8', {
    ignoreBOM: true,
    fatal: true
  });
  cachedTextDecoder.decode();

  function getStringFromWasm(ptr, len) {
    return cachedTextDecoder.decode(getUint8Memory().subarray(ptr, ptr + len));
  }

  let cachedTextEncoder = new TextEncoder('utf-8');
  const encodeString = typeof cachedTextEncoder.encodeInto === 'function' ? function (arg, view) {
    return cachedTextEncoder.encodeInto(arg, view);
  } : function (arg, view) {
    const buf = cachedTextEncoder.encode(arg);
    view.set(buf);
    return {
      read: arg.length,
      written: buf.length
    };
  };

  function passStringToWasm(arg) {
    let len = arg.length;

    let ptr = wasm.__wbindgen_malloc(len);

    const mem = getUint8Memory();
    let offset = 0;

    for (; offset < len; offset++) {
      const code = arg.charCodeAt(offset);
      if (code > 0x7F) break;
      mem[ptr + offset] = code;
    }

    if (offset !== len) {
      if (offset !== 0) {
        arg = arg.slice(offset);
      }

      ptr = wasm.__wbindgen_realloc(ptr, len, len = offset + arg.length * 3);
      const view = getUint8Memory().subarray(ptr + offset, ptr + len);
      const ret = encodeString(arg, view);
      offset += ret.written;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
  }

  let cachegetInt32Memory = null;

  function getInt32Memory() {
    if (cachegetInt32Memory === null || cachegetInt32Memory.buffer !== wasm.memory.buffer) {
      cachegetInt32Memory = new Int32Array(wasm.memory.buffer);
    }

    return cachegetInt32Memory;
  }
  /**
  */


  class FrameHeaderV2 {
    static __wrap(ptr) {
      const obj = Object.create(FrameHeaderV2.prototype);
      obj.ptr = ptr;
      return obj;
    }

    free() {
      const ptr = this.ptr;
      this.ptr = 0;

      wasm.__wbg_frameheaderv2_free(ptr);
    }
    /**
    * @returns {number}
    */


    get time_on() {
      const ret = wasm.__wbg_get_frameheaderv2_time_on(this.ptr);

      return ret >>> 0;
    }
    /**
    * @param {number} arg0
    */


    set time_on(arg0) {
      wasm.__wbg_set_frameheaderv2_time_on(this.ptr, arg0);
    }
    /**
    * @returns {number}
    */


    get last_ffc_time() {
      const ret = wasm.__wbg_get_frameheaderv2_last_ffc_time(this.ptr);

      return ret >>> 0;
    }
    /**
    * @param {number} arg0
    */


    set last_ffc_time(arg0) {
      wasm.__wbg_set_frameheaderv2_last_ffc_time(this.ptr, arg0);
    }
    /**
    * @returns {number}
    */


    get frame_number() {
      const ret = wasm.__wbg_get_frameheaderv2_frame_number(this.ptr);

      return ret >>> 0;
    }
    /**
    * @param {number} arg0
    */


    set frame_number(arg0) {
      wasm.__wbg_set_frameheaderv2_frame_number(this.ptr, arg0);
    }
    /**
    * @returns {boolean}
    */


    get has_next_frame() {
      const ret = wasm.__wbg_get_frameheaderv2_has_next_frame(this.ptr);

      return ret !== 0;
    }
    /**
    * @param {boolean} arg0
    */


    set has_next_frame(arg0) {
      wasm.__wbg_set_frameheaderv2_has_next_frame(this.ptr, arg0);
    }

  }

  __exports.FrameHeaderV2 = FrameHeaderV2;

  function init(module) {
    let result;
    const imports = {};
    imports.wbg = {};

    imports.wbg.__wbindgen_object_drop_ref = function (arg0) {
      takeObject(arg0);
    };

    imports.wbg.__wbindgen_string_new = function (arg0, arg1) {
      const ret = getStringFromWasm(arg0, arg1);
      return addHeapObject(ret);
    };

    imports.wbg.__widl_f_debug_1_ = function (arg0) {
      console.debug(getObject(arg0));
    };

    imports.wbg.__widl_f_error_1_ = function (arg0) {
      console.error(getObject(arg0));
    };

    imports.wbg.__widl_f_info_1_ = function (arg0) {
      console.info(getObject(arg0));
    };

    imports.wbg.__widl_f_log_1_ = function (arg0) {
      console.log(getObject(arg0));
    };

    imports.wbg.__widl_f_warn_1_ = function (arg0) {
      console.warn(getObject(arg0));
    };

    imports.wbg.__wbg_new_59cb74e423758ede = function () {
      const ret = new Error();
      return addHeapObject(ret);
    };

    imports.wbg.__wbg_stack_558ba5917b466edd = function (arg0, arg1) {
      const ret = getObject(arg1).stack;
      const ret0 = passStringToWasm(ret);
      const ret1 = WASM_VECTOR_LEN;
      getInt32Memory()[arg0 / 4 + 0] = ret0;
      getInt32Memory()[arg0 / 4 + 1] = ret1;
    };

    imports.wbg.__wbg_error_4bb6c2a97407129a = function (arg0, arg1) {
      const v0 = getStringFromWasm(arg0, arg1).slice();

      wasm.__wbindgen_free(arg0, arg1 * 1);

      console.error(v0);
    };

    imports.wbg.__wbindgen_throw = function (arg0, arg1) {
      throw new Error(getStringFromWasm(arg0, arg1));
    };

    imports.wbg.__wbindgen_rethrow = function (arg0) {
      throw takeObject(arg0);
    };

    if (typeof URL === 'function' && module instanceof URL || typeof module === 'string' || typeof Request === 'function' && module instanceof Request) {
      const response = fetch(module);

      if (typeof WebAssembly.instantiateStreaming === 'function') {
        result = WebAssembly.instantiateStreaming(response, imports).catch(e => {
          return response.then(r => {
            if (r.headers.get('Content-Type') != 'application/wasm') {
              console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);
              return r.arrayBuffer();
            } else {
              throw e;
            }
          }).then(bytes => WebAssembly.instantiate(bytes, imports));
        });
      } else {
        result = response.then(r => r.arrayBuffer()).then(bytes => WebAssembly.instantiate(bytes, imports));
      }
    } else {
      result = WebAssembly.instantiate(module, imports).then(result => {
        if (result instanceof WebAssembly.Instance) {
          return {
            instance: result,
            module
          };
        } else {
          return result;
        }
      });
    }

    return result.then(({
      instance,
      module
    }) => {
      wasm = instance.exports;
      init.__wbindgen_wasm_module = module;
      return wasm;
    });
  }

  self.wasm_bindgen = Object.assign(init, __exports);
})();

/* harmony default export */ var cptv_player = (self.wasm_bindgen);
// EXTERNAL MODULE: ./node_modules/worker-loader/dist/cjs.js!./src/smoothing-worker.ts
var smoothing_worker = __webpack_require__("4847");
var smoothing_worker_default = /*#__PURE__*/__webpack_require__.n(smoothing_worker);

// CONCATENATED MODULE: ./node_modules/cache-loader/dist/cjs.js??ref--13-0!./node_modules/thread-loader/dist/cjs.js!./node_modules/babel-loader/lib!./node_modules/ts-loader??ref--13-3!./src/frame-listener.ts
function ownKeys(object, enumerableOnly) { var keys = Object.keys(object); if (Object.getOwnPropertySymbols) { var symbols = Object.getOwnPropertySymbols(object); if (enumerableOnly) symbols = symbols.filter(function (sym) { return Object.getOwnPropertyDescriptor(object, sym).enumerable; }); keys.push.apply(keys, symbols); } return keys; }

function _objectSpread(target) { for (var i = 1; i < arguments.length; i++) { var source = arguments[i] != null ? arguments[i] : {}; if (i % 2) { ownKeys(Object(source), true).forEach(function (key) { _defineProperty(target, key, source[key]); }); } else if (Object.getOwnPropertyDescriptors) { Object.defineProperties(target, Object.getOwnPropertyDescriptors(source)); } else { ownKeys(Object(source)).forEach(function (key) { Object.defineProperty(target, key, Object.getOwnPropertyDescriptor(source, key)); }); } } return target; }

function _defineProperty(obj, key, value) { if (key in obj) { Object.defineProperty(obj, key, { value: value, enumerable: true, configurable: true, writable: true }); } else { obj[key] = value; } return obj; }

 // import { WasmTracingAllocator } from "@/tracing-allocator";



const {
  initWithCptvData,
  getRawFrame
} = cptv_player;
const smoothingWorkers = [{
  worker: new smoothing_worker_default.a(),
  pending: null,
  index: 0
}];

for (let i = 0; i < smoothingWorkers.length; i++) {
  const s = smoothingWorkers[i];

  s.worker.onmessage = result => {
    if (s.pending) {
      // TODO(jon): See if we're ever getting frame number mis-matches here.
      s.pending(result.data);
      s.pending = null;
    } else {
      console.error("Couldn't find callback for", result.data, s.index);
    }
  };
}

let workerIndex = 0;
const processSensorData = async frame => {
  const index = workerIndex;
  return new Promise((resolve, reject) => {
    smoothingWorkers[index].pending = resolve;
    smoothingWorkers[index].worker.postMessage({
      frame: frame.frame,
      calibrationTempC: frame.frameInfo.Calibration.TemperatureCelsius
    });
  });
};
const workerContext = self;
let frameTimeout = 0;
let frameBuffer = null;
const InitialFrameInfo = {
  Camera: {
    ResX: 160,
    ResY: 120,
    FPS: 9,
    Brand: "flir",
    Model: "lepton3.5",
    Firmware: "3.3.26",
    CameraSerial: 12345
  },
  Telemetry: {
    FrameCount: 1,
    TimeOn: 1,
    FFCState: "On",
    FrameMean: 0,
    TempC: 0,
    LastFFCTempC: 0,
    LastFFCTime: 0
  },
  AppVersion: "",
  BinaryVersion: "",
  Calibration: {
    ThermalRefTemp: 38,
    SnapshotTime: 0,
    TemperatureCelsius: 34,
    SnapshotValue: 0,
    ThresholdMinFever: 0,
    Bottom: 0,
    Top: 0,
    Left: 0,
    Right: 0,
    CalibrationBinaryVersion: "fsdfd",
    UuidOfUpdater: 432423432432,
    UseErrorSound: true,
    UseWarningSound: true,
    UseNormalSound: true
  }
};

async function processFrame(frame) {
  // console.log("got frame", frame);
  // Do the frame processing, then postMessage the relevant payload to the view app.
  // Do this in yet another worker(s)?
  performance.mark(`process frame ${frame.frameInfo.Telemetry.FrameCount}`);
  const imageInfo = await processSensorData(frame);
  performance.mark(`end frame ${frame.frameInfo.Telemetry.FrameCount}`);
  performance.measure(`frame ${frame.frameInfo.Telemetry.FrameCount}`, `start frame ${frame.frameInfo.Telemetry.FrameCount}`, `end frame ${frame.frameInfo.Telemetry.FrameCount}`);
  performance.measure(`process frame ${frame.frameInfo.Telemetry.FrameCount}`, `process frame ${frame.frameInfo.Telemetry.FrameCount}`, `end frame ${frame.frameInfo.Telemetry.FrameCount}`);
  workerContext.postMessage({
    type: "gotFrame",
    payload: {
      frameInfo: frame.frameInfo,
      frame: frame.frame,
      bodyShape: imageInfo.bodyShape,
      analysisResult: imageInfo.analysisResult
    }
  });
}

function onConnectionStateChange(connectionState) {
  workerContext.postMessage({
    type: "connectionStateChange",
    payload: connectionState
  });
}

function getNextFrame(startFrame = -1, endFrame = -1) {
  let frameInfo = getRawFrame(frameBuffer);

  while (frameInfo.frame_number < startFrame || endFrame != -1 && frameInfo.frame_number > endFrame) {
    frameInfo = getRawFrame(frameBuffer);
  }

  const appVersion = "";
  const binaryVersion = "";
  const currentFrame = {
    frame: new Uint16Array(frameBuffer.buffer),
    frameInfo: _objectSpread(_objectSpread({}, InitialFrameInfo), {}, {
      AppVersion: appVersion,
      BinaryVersion: binaryVersion,
      Telemetry: _objectSpread(_objectSpread({}, InitialFrameInfo.Telemetry), {}, {
        LastFFCTime: frameInfo.last_ffc_time,
        FrameCount: frameInfo.frame_number,
        TimeOn: frameInfo.time_on
      })
    })
  };
  frameInfo.free();
  frameTimeout = setTimeout(getNextFrame, 1000 / 9);
  const frameNumber = currentFrame.frameInfo.Telemetry.FrameCount;

  if (frameNumber % 20 === 0) {
    performance.clearMarks();
    performance.clearMeasures();
    performance.clearResourceTimings();
  }

  performance.mark(`start frame ${frameNumber}`);
  processFrame(currentFrame);
}

function playLocalCptvFile(cptvFileBytes, startFrame = 0, endFrame = -1) {
  frameBuffer = new Uint8Array(160 * 120 * 2);
  initWithCptvData(new Uint8Array(cptvFileBytes));
  getNextFrame();
}

(async function run() {
  workerContext.addEventListener("message", async event => {
    const message = event.data; // if (message.dumpMemoryAllocations) {
    //   WasmTracingAllocator.dumpInvalidFrees();
    // }

    if (message.useLiveCamera) {
      new camera_CameraConnection(message.hostname, message.port, processFrame, onConnectionStateChange); // Init live camera web-socket connection
    } else if (message.cptvFileToPlayback) {
      // Init CPTV file playback
      await cptv_player(`${"/static/dist/"}cptv_player_bg.wasm`);
      const cptvFile = await fetch(message.cptvFileToPlayback);
      const buffer = await cptvFile.arrayBuffer();
      playLocalCptvFile(buffer, message.startFrame || 0, message.endFrame || -1);
    }

    return;
  });
})();
/*
    this.useLiveCamera = false;
    if (this.useLiveCamera) {
      // FIXME(jon): Add the proper camera url
      // FIXME(jon): Get rid of browser full screen toggle
      new CameraConnection(
        window.location.hostname,
        this.onFrame,
        this.onConnectionStateChange
      );
    } else {
      // TODO(jon): Queue multiple files
      cptvPlayer = await import("../cptv-player/cptv_player");
      ///const cptvFile = await fetch("/cptv-files/twopeople-calibration.cptv");
      //const cptvFile = await fetch();
      //"cptv-files/bunch of people in small meeting room 20200812.134427.735.cptv",
      //"/cptv-files/bunch of people downstairs walking towards camera 20200812.161144.768.cptv"
      const cptvFile = await fetch(
        "/cptv-files/0.7.5beta recording-1 2708.cptv"
      ); //
      //const cptvFile = await fetch("/cptv-files/20200716.153342.441.cptv");
      //const cptvFile = await fetch("/cptv-files/20200716.153342.441.cptv"); // Jon (too high in frame)
      //const cptvFile = await fetch("/cptv-files/20200718.130624.941.cptv"); // Sara

      //const cptvFile = await fetch("/cptv-files/20200718.130606.382.cptv"); // Sara
      //const cptvFile = await fetch("/cptv-files/20200718.130536.950.cptv"); // Sara (fringe)
      //const cptvFile = await fetch("/cptv-files/20200718.130508.586.cptv"); // Sara (fringe)
      //const cptvFile = await fetch("/cptv-files/20200718.130059.393.cptv"); // Jon
      //const cptvFile = await fetch("/cptv-files/20200718.130017.220.cptv"); // Jon
      //

      //const cptvFile = await fetch("/cptv-files/walking through Shaun.cptv");
      //const cptvFile = await fetch("/cptv-files/looking_down.cptv");
      // const cptvFile = await fetch(
      //   "/cptv-files/detecting part then whole face repeatedly.cptv"
      // );
      //frontend\public\cptv-files\detecting part then whole face repeatedly.cptv
      // const cptvFile = await fetch(
      //   "/cptv-files/walking towards camera - calibrated at 2m.cptv"
      // );

      // Shauns office:
      //const cptvFile = await fetch("/cptv-files/20200729.104543.646.cptv");
      //const cptvFile = await fetch("/cptv-files/20200729.104622.519.cptv");
      //const cptvFile = await fetch("/cptv-files/20200729.104815.556.cptv"); // Proximity

      //const cptvFile = await fetch("/cptv-files/20200729.105022.389.cptv");
      // 20200729.105038.847
      //const cptvFile = await fetch("/cptv-files/20200729.105038.847.cptv");
      //const cptvFile = await fetch("/cptv-files/20200729.105053.858.cptv");
      const buffer = await cptvFile.arrayBuffer();
      // 30, 113, 141
      await this.playLocalCptvFile(buffer, this.startFrame);
    }
    */

/***/ })

/******/ });
//# sourceMappingURL=373944890a77448c5eb8.worker.js.map