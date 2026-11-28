// Under water with ray tracing (`render::murk`): the water's murk laid over
// the picture by how far away each pixel is, since the ray traced lighting
// leaves out the distance fog the other tiers close in with.

struct FullscreenVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Murk {
    // the murk's colour, as the distance fog has it
    colour: vec4<f32>,
    // where it starts and where it's whole, the camera's near plane, and
    // how far the water's surface is over the eye
    reach: vec4<f32>,
    // the view's half width and half height one unit in front of the eye
    ray: vec4<f32>,
    // the camera's right, up and back, in the world
    right: vec4<f32>,
    up: vec4<f32>,
    back: vec4<f32>,
};

@group(0) @binding(0) var depth: texture_depth_2d;
@group(0) @binding(1) var<uniform> murk: Murk;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(in.position.xy), 0);
    let ndc = in.uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    // the pixel's ray, reaching one unit forward
    let v = vec3(ndc.x * murk.ray.x, ndc.y * murk.ray.y, -1.0);
    let along = length(v);
    // reverse depth with no far plane: the near plane over how far forward
    var dist = select(1.0e6, murk.reach.z / max(d, 1.0e-9), d > 0.0) * along;
    // past the water's surface is air: the murk ends there
    let up = dot(murk.right.xyz * v.x + murk.up.xyz * v.y + murk.back.xyz * v.z, vec3(0.0, 1.0, 0.0));
    if up > 1.0e-4 {
        dist = min(dist, murk.reach.w / up * along);
    }
    let f = clamp((dist - murk.reach.x) / max(murk.reach.y - murk.reach.x, 1.0e-3), 0.0, 1.0);
    return vec4(murk.colour.rgb, f * murk.colour.a);
}
