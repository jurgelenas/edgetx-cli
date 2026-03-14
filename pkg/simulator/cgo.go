package simulator

// CGO flags for the simulator package.
// The WAMR headers and libraries are provided by the wamr Go package.
// We reference them via the go module cache path using SRCDIR-relative paths.
//
// The wamr package's cgo.go already sets CFLAGS and LDFLAGS but those only
// apply when compiling the wamr package itself. For our package, we need to
// duplicate the include path.
//
// The packaged include directory must contain wasm_export.h. If building
// from source, copy headers from the WAMR repo:
//   core/iwasm/include/wasm_export.h -> language-bindings/go/wamr/packaged/include/

// #cgo CFLAGS: -I${SRCDIR}/../../vendor_include/wamr
import "C"
