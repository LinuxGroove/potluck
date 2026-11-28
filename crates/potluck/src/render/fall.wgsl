// Rain and snow falling round the player (`render::fall`). Each drop or
// flake is a quad whose four corners share a place in a box of the world
// round the eye; the box wraps, so they fall for ever and the eye never
// reaches its edge. A drop is a thin streak along the way it falls, turned
// to face the eye; a flake a soft round dot that drifts from side to side.
// Nothing falls under a roof, a tree or the water's surface (the roof map),
// right at the eye, or past what's falling now.

struct Fall {
    // the camera's clip space from the world's
    view_proj: mat4x4<f32>,
    // the eye, and the time in seconds
    eye: vec4<f32>,
    // the wind along x and z, how fast it falls, how much of it falls (0 to 1)
    wind: vec4<f32>,
    // its colour and how see-through
    look: vec4<f32>,
    // the box's size, a drop's length and width, 1 for snow
    shape: vec4<f32>,
    // the roof map's corner in x and z, and how many blocks across it is
    roofs: vec4<f32>,
};

@group(3) @binding(0) var<uniform> fall: Fall;
@group(3) @binding(1) var roofs: texture_2d<f32>;

struct Vertex {
    // where in the box, 0 to 1 each way
    @location(0) position: vec3<f32>,
    // the corner: across (-0.5 to 0.5) and along (0 to 1)
    @location(2) uv: vec2<f32>,
    // which share of the most it falls in, and a phase for drifting
    @location(3) uv_b: vec2<f32>,
};

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fade: f32,
};

@vertex
fn vertex(v: Vertex) -> Out {
    var o: Out;
    let size = fall.shape.x;
    let t = fall.eye.w;
    let snow = fall.shape.w;
    let vel = vec3(fall.wind.x, -fall.wind.z, fall.wind.y);
    var p = v.position * size + vel * t;
    // flakes wander a little as they fall
    let phase = v.uv_b.y * 6.2832;
    p += snow * vec3(sin(t * 0.9 + phase), 0.0, cos(t * 0.7 + phase * 1.3)) * 0.35;
    let rel = p - fall.eye.xyz;
    let wrapped = rel - size * floor(rel / size + 0.5);
    let at = fall.eye.xyz + wrapped;
    let dist = length(wrapped);
    // thinning out toward the box's edge, and never in the eye
    let out = clamp(dist / (size * 0.5), 0.0, 1.0);
    var fade = (1.0 - out * out) * smoothstep(0.4, 1.4, dist);
    if (v.uv_b.x > fall.wind.w) {
        fade = 0.0;
    }
    let n = i32(fall.roofs.z);
    let cell = vec2<i32>(floor(at.xz - fall.roofs.xy));
    if (cell.x >= 0 && cell.y >= 0 && cell.x < n && cell.y < n) {
        if (at.y < textureLoad(roofs, cell, 0).r) {
            fade = 0.0;
        }
    }
    let to_eye = normalize(fall.eye.xyz - at);
    var along = normalize(vel);
    var side = cross(along, to_eye);
    if (length(side) < 1e-4) {
        side = vec3(1.0, 0.0, 0.0);
    }
    side = normalize(side);
    if (snow > 0.5) {
        // a flake faces the eye
        along = normalize(cross(to_eye, side));
    }
    let corner = at + side * v.uv.x * fall.shape.z - along * (v.uv.y - 0.5) * fall.shape.y;
    o.clip = fall.view_proj * vec4(corner, 1.0);
    if (fade <= 0.0) {
        // all four corners at one place outside the view: nothing drawn
        o.clip = vec4(10.0, 10.0, 10.0, 1.0);
    }
    o.uv = v.uv;
    o.fade = fade;
    return o;
}

@fragment
fn fragment(i: Out) -> @location(0) vec4<f32> {
    var a = fall.look.a * i.fade;
    if (fall.shape.w > 0.5) {
        // round and soft
        let d = length(vec2(i.uv.x * 2.0, i.uv.y * 2.0 - 1.0));
        a *= clamp(1.0 - d * d, 0.0, 1.0);
    } else {
        // thin at both ends and at the edges
        a *= sin(i.uv.y * 3.14159) * (1.0 - abs(i.uv.x) * 2.0);
    }
    return vec4(fall.look.rgb, a);
}
