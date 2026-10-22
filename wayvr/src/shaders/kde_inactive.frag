#version 310 es
precision highp float;
layout (location = 0) in vec2 in_uv;
layout (location = 0) out vec4 out_color;
layout (set = 0, binding = 0) uniform sampler2D in_texture;
void main() {
    bool border = min(min(in_uv.x, 1.0-in_uv.x), min(in_uv.y, 1.0-in_uv.y)) < 0.012;
    out_color = border ? vec4(1.0, 0.3, 0.0, 1.0) : vec4(texture(in_texture, in_uv).rgb * 0.65, 1.0);
}
