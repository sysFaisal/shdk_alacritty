#ifdef GLES2_RENDERER
precision mediump float;
varying vec2 fragPos;
#else
in vec2 fragPos;
out vec4 color;
#endif

uniform vec4 cursorEdges;
uniform vec3 trailColor;
uniform float trailOpacity;

void main() {
    float in_x = step(cursorEdges.x, fragPos.x) * step(fragPos.x, cursorEdges.y);
    float in_y = step(cursorEdges.w, fragPos.y) * step(fragPos.y, cursorEdges.z);
    float opacity = trailOpacity * (1.0 - in_x * in_y);
    vec4 outputColor = vec4(trailColor * opacity, opacity);
#ifdef GLES2_RENDERER
    gl_FragColor = outputColor;
#else
    color = outputColor;
#endif
}
