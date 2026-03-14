package simulator

// The wamr Go package (imported in runtime.go) provides CGO flags for the
// WAMR include path and library. This file adds the C functions we need that
// aren't exposed by the official Go bindings: register_natives and helpers.
//
// The wasm_export.h header is found via the wamr package's CGO CFLAGS.
// We use `#cgo pkg-config:` or rely on the transitive CGO flags from the
// wamr package. If that doesn't work, the WAMR_INCLUDE environment variable
// can be set to the packaged/include directory.

/*
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>

#include <wasm_export.h>

// ---- Host import trampolines ----
// These C functions are called by WAMR when the WASM module invokes a host import.
// They call the Go exported functions which look up the active Runtime.

extern uint32_t goSimuGetAnalog(wasm_exec_env_t exec_env, uint32_t idx);
extern void goSimuQueueAudio(wasm_exec_env_t exec_env, uint32_t buf_ptr, uint32_t len);
extern void goSimuTrace(wasm_exec_env_t exec_env, uint32_t text_ptr);
extern void goSimuLcdNotify(wasm_exec_env_t exec_env);

static uint32_t
trampoline_simuGetAnalog(wasm_exec_env_t exec_env, uint32_t idx)
{
    return goSimuGetAnalog(exec_env, idx);
}

static void
trampoline_simuQueueAudio(wasm_exec_env_t exec_env, uint32_t buf_ptr, uint32_t len)
{
    goSimuQueueAudio(exec_env, buf_ptr, len);
}

static void
trampoline_simuTrace(wasm_exec_env_t exec_env, uint32_t text_ptr)
{
    goSimuTrace(exec_env, text_ptr);
}

static void
trampoline_simuLcdNotify(wasm_exec_env_t exec_env)
{
    goSimuLcdNotify(exec_env);
}

// Native symbol table for the "env" module.
static NativeSymbol env_symbols[] = {
    {"simuGetAnalog",  (void*)trampoline_simuGetAnalog,  "(i)i", NULL},
    {"simuQueueAudio", (void*)trampoline_simuQueueAudio, "(ii)",  NULL},
    {"simuTrace",      (void*)trampoline_simuTrace,      "(i)",   NULL},
    {"simuLcdNotify",  (void*)trampoline_simuLcdNotify,  "()",    NULL},
};

static bool register_env_natives() {
    return wasm_runtime_register_natives(
        "env",
        env_symbols,
        sizeof(env_symbols) / sizeof(NativeSymbol)
    );
}


// Clear exception on a module instance.
static void clear_exception(void *inst_ptr) {
    wasm_module_inst_t inst = (wasm_module_inst_t)inst_ptr;
    wasm_runtime_clear_exception(inst);
}

// Create a new exec_env for calling WASM from a different thread.
static void* create_exec_env(void *inst_ptr) {
    wasm_module_inst_t inst = (wasm_module_inst_t)inst_ptr;
    return (void*)wasm_runtime_create_exec_env(inst, 32768);
}

static void destroy_exec_env(void *exec_env_ptr) {
    wasm_runtime_destroy_exec_env((wasm_exec_env_t)exec_env_ptr);
}

// Call a WASM function using a specific exec_env.
// Returns the first result arg, or -1 on lookup failure, -2 on call failure.
static int32_t call_wasm_with_env(void *inst_ptr, void *exec_env_ptr,
                                   const char *func_name,
                                   uint32_t argc, uint32_t argv[]) {
    wasm_module_inst_t inst = (wasm_module_inst_t)inst_ptr;
    wasm_exec_env_t exec_env = (wasm_exec_env_t)exec_env_ptr;

    wasm_function_inst_t func = wasm_runtime_lookup_function(inst, func_name);
    if (!func) return -1;

    if (!wasm_runtime_call_wasm(exec_env, func, argc, argv)) {
        wasm_runtime_clear_exception(inst);
        return -2;
    }
    return (argc > 0) ? (int32_t)argv[0] : 0;
}

// Set WASI args with preopened directories.
// IMPORTANT: wasm_runtime_set_wasi_args stores pointers, not copies.
// Both the dir strings AND the dirs array must remain valid until
// after wasm_runtime_instantiate is called.
// Returns the heap-allocated dirs array (caller must free after instantiation).
static void* set_wasi_args_2dirs(void *module_ptr, const char *dir1, const char *dir2) {
    wasm_module_t module = (wasm_module_t)module_ptr;
    // Heap-allocate the array so it survives past this function call.
    const char **dirs = (const char **)malloc(2 * sizeof(const char *));
    dirs[0] = dir1;
    dirs[1] = dir2;
    wasm_runtime_set_wasi_args(module, dirs, 2, NULL, 0, NULL, 0, NULL, 0);
    return (void*)dirs;
}
*/
import "C"
import (
	"fmt"
	"unsafe"

	"github.com/bytecodealliance/wasm-micro-runtime/language-bindings/go/wamr"
)

// clearException clears the exception state on a WAMR instance.
func clearException(inst *wamr.Instance) {
	rawPtr := *(*unsafe.Pointer)(unsafe.Pointer(inst))
	C.clear_exception(rawPtr)
}

// registerEnvNatives registers the 4 host import functions with WAMR.
// Must be called after wamr.Runtime().Init() and before module instantiation.
func registerEnvNatives() error {
	if !C.register_env_natives() {
		return fmt.Errorf("failed to register env native symbols")
	}
	return nil
}

// ExecEnv wraps a WAMR exec_env for calling WASM from a non-main thread.
type ExecEnv struct {
	instPtr unsafe.Pointer
	envPtr  unsafe.Pointer
}

// CreateExecEnv creates a new exec_env bound to the current OS thread.
func CreateExecEnv(inst *wamr.Instance) (*ExecEnv, error) {
	instPtr := *(*unsafe.Pointer)(unsafe.Pointer(inst))
	envPtr := C.create_exec_env(instPtr)
	if envPtr == nil {
		return nil, fmt.Errorf("failed to create exec_env")
	}
	return &ExecEnv{instPtr: instPtr, envPtr: envPtr}, nil
}

// Destroy releases the exec_env.
func (e *ExecEnv) Destroy() {
	if e.envPtr != nil {
		C.destroy_exec_env(e.envPtr)
		e.envPtr = nil
	}
}

// Call calls a WASM export function using this exec_env.
func (e *ExecEnv) Call(funcName string, argc uint32, args []uint32) (int32, error) {
	cName := C.CString(funcName)
	defer C.free(unsafe.Pointer(cName))

	var cArgs *C.uint32_t
	if argc > 0 {
		cArgs = (*C.uint32_t)(unsafe.Pointer(&args[0]))
	}

	ret := int32(C.call_wasm_with_env(e.instPtr, e.envPtr, cName, C.uint32_t(argc), cArgs))
	if ret == -1 {
		return 0, fmt.Errorf("function %s not found", funcName)
	}
	if ret == -2 {
		return 0, fmt.Errorf("call %s failed", funcName)
	}
	return ret, nil
}

// wasiDirKeepAlive holds C-allocated strings and the dirs array that must
// outlive SetWasiArgs until after module instantiation (WAMR stores pointers, not copies).
var (
	wasiDirKeepAlive      []*C.char
	wasiDirArrayKeepAlive unsafe.Pointer
)

// setWasiPreopens configures WASI preopened directories using pure C calls,
// bypassing the wamr Go bindings' SetWasiArgs which has CGO pointer issues.
// The C strings are kept alive until freeWasiDirs is called.
func setWasiPreopens(module *wamr.Module, sdcardDir, settingsDir string) {
	// Extract the raw C pointer from the Module struct (single pointer-sized field).
	// wamr.Module = struct { module C.wasm_module_t } where wasm_module_t is a C pointer.
	rawPtr := *(*unsafe.Pointer)(unsafe.Pointer(module))

	cSD := C.CString(sdcardDir)
	cSettings := C.CString(settingsDir)

	// Keep references alive — WAMR stores pointers, not copies.
	wasiDirKeepAlive = append(wasiDirKeepAlive, cSD, cSettings)

	dirsArray := C.set_wasi_args_2dirs(rawPtr, cSD, cSettings)
	// Keep the dirs array alive too.
	wasiDirArrayKeepAlive = unsafe.Pointer(dirsArray)
}

// freeWasiDirs releases the C strings kept alive for WASI preopens.
// Call after the WASM instance is destroyed.
func freeWasiDirs() {
	for _, p := range wasiDirKeepAlive {
		C.free(unsafe.Pointer(p))
	}
	wasiDirKeepAlive = nil
	if wasiDirArrayKeepAlive != nil {
		C.free(wasiDirArrayKeepAlive)
		wasiDirArrayKeepAlive = nil
	}
}
