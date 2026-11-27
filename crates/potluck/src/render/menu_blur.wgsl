// The world behind a menu, softened and a little darker (`render::MenuBlur`):
// two rings of taps round each pixel, as wide as the window is tall allows.

struct FullscreenVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct MenuBlur {
    amount: f32,
    _a: f32,
    _b: f32,
    _c: f32,
};

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> blur: MenuBlur;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(screen_texture));
    let texel = 1.0 / size;
    let r = blur.amount * size.y / 1080.0 * 3.5;
    let centre = textureSample(screen_texture, screen_sampler, in.uv);
    var sum = centre * 0.16;
    for (var i = 0; i < 8; i++) {
        let a = f32(i) * 0.785398;
        let inner = vec2<f32>(cos(a), sin(a)) * r;
        let outer = vec2<f32>(cos(a + 0.392699), sin(a + 0.392699)) * r * 2.1;
        sum += textureSample(screen_texture, screen_sampler, in.uv + inner * texel) * 0.07;
        sum += textureSample(screen_texture, screen_sampler, in.uv + outer * texel) * 0.035;
    }
    let dim = 1.0 - 0.14 * blur.amount;
    return vec4<f32>(sum.rgb * dim, centre.a);
}
