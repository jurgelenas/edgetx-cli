package simulator

/*
#cgo LDFLAGS: -lGL
#include <GL/gl.h>

// Use glTexImage2D (full re-upload) instead of glTexSubImage2D.
// This avoids potential issues with texture state in newer ImGui.
void updateTextureRGBA(unsigned int texID, int width, int height, const unsigned char *pixels) {
    GLint last_texture;
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &last_texture);
    glBindTexture(GL_TEXTURE_2D, texID);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, width, height, 0, GL_RGBA, GL_UNSIGNED_BYTE, pixels);
    glBindTexture(GL_TEXTURE_2D, last_texture);
}
*/
import "C"
import (
	"unsafe"

	"github.com/AllenDang/cimgui-go/imgui"
)

// updateTexture updates an existing OpenGL texture with new RGBA pixel data.
func updateTexture(tex imgui.TextureRef, width, height int, pixels []byte) {
	texID := uint32(tex.TexID())
	C.updateTextureRGBA(C.uint(texID), C.int(width), C.int(height), (*C.uchar)(unsafe.Pointer(&pixels[0])))
}
