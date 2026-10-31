#if defined(GLES2_RENDERER)
precision mediump float;
varying mediump vec2 TexCoord;
uniform sampler2D imageTexture;
#define FRAG_COLOR gl_FragColor
#define TEXTURE texture2D
#else
in vec2 TexCoord;
out vec4 FragColor;
uniform sampler2D imageTexture;
#define FRAG_COLOR FragColor
#define TEXTURE texture
#endif

void main() {
    FRAG_COLOR = TEXTURE(imageTexture, TexCoord);
}
