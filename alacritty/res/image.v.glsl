#if defined(GLES2_RENDERER)
attribute vec2 aPos;
attribute vec2 aTexCoord;
varying mediump vec2 TexCoord;
#else
layout (location = 0) in vec2 aPos;
layout (location = 1) in vec2 aTexCoord;
out vec2 TexCoord;
#endif

void main() {
    TexCoord = aTexCoord;
    gl_Position = vec4(aPos.x, aPos.y, 0.0, 1.0);
}
