#ifdef GLES2_RENDERER
attribute vec2 position;
varying vec2 fragPos;
#else
layout(location = 0) in vec2 position;
out vec2 fragPos;
#endif

void main() {
    gl_Position = vec4(position, 0.0, 1.0);
    fragPos = position;
}
