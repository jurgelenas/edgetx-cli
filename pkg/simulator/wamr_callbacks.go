package simulator

/*
#include <stdint.h>
#include <wasm_export.h>
*/
import "C"
import (
	"unsafe"
)

//export goSimuGetAnalog
func goSimuGetAnalog(execEnv C.wasm_exec_env_t, idx C.uint32_t) C.uint32_t {
	rt := activeRuntime
	if rt == nil {
		return 2048 // center default
	}
	i := int(idx)
	if i < 0 || i >= len(rt.analogValues) {
		return 2048
	}
	return C.uint32_t(rt.analogValues[i])
}

//export goSimuQueueAudio
func goSimuQueueAudio(execEnv C.wasm_exec_env_t, bufPtr C.uint32_t, length C.uint32_t) {
	rt := activeRuntime
	if rt == nil {
		return
	}

	instPtr := C.wasm_runtime_get_module_inst(execEnv)
	nativePtr := C.wasm_runtime_addr_app_to_native(
		instPtr,
		C.uint64_t(bufPtr),
	)
	if nativePtr == nil {
		return
	}

	// Copy audio data from WASM memory.
	numSamples := int(length) / 2 // S16 = 2 bytes per sample
	if numSamples <= 0 {
		return
	}
	src := unsafe.Slice((*int16)(nativePtr), numSamples)
	samples := make([]int16, numSamples)
	copy(samples, src)

	// Non-blocking send to audio queue.
	select {
	case rt.audioQueue <- samples:
	default:
		// Drop audio if queue is full.
	}
}

//export goSimuTrace
func goSimuTrace(execEnv C.wasm_exec_env_t, textPtr C.uint32_t) {
	rt := activeRuntime
	if rt == nil {
		return
	}

	instPtr := C.wasm_runtime_get_module_inst(execEnv)
	nativePtr := C.wasm_runtime_addr_app_to_native(
		instPtr,
		C.uint64_t(textPtr),
	)
	if nativePtr == nil {
		return
	}

	// Read null-terminated string from WASM memory.
	str := C.GoString((*C.char)(nativePtr))
	rt.traceWriter.Write([]byte(str))
	rt.traceWriter.Write([]byte("\n"))
}

//export goSimuLcdNotify
func goSimuLcdNotify(execEnv C.wasm_exec_env_t) {
	rt := activeRuntime
	if rt == nil {
		return
	}

	// Non-blocking signal.
	select {
	case rt.lcdReady <- struct{}{}:
	default:
	}
}
