// River and lake surface. Self-contained: everything it needs (camera, time, sun, sky colour)
// comes in through the material's own uniform, set by `water_update` in src/main.rs.
//
// The surface is a height field made of several layers of drifting noise; its slope gives a
// normal per pixel, which drives reflection of the sky (Fresnel), glitter from the sun or moon,
// and the shading of the water body itself.

struct Water {
    cam: vec4<f32>,   // camera position, w = time in seconds
    sun: vec4<f32>,   // direction towards the sun (or moon), w = daylight 0..1
    sky: vec4<f32>,   // colour of the sky reflected in the water, w = rain 0..1
    sunc: vec4<f32>,  // colour and strength of the sun / moon glitter, w = wind strength 0..1
    deep: vec4<f32>,  // colour of the water body, w unused
    flow: vec4<f32>,  // xy = direction the river drifts, zw unused
};

@group(3) @binding(0) var<uniform> w: Water;

struct In {
    @builtin(position) frag: vec4<f32>,
    @location(0) world: vec4<f32>,
};

fn hash(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(0.1031, 0.1030));
    q = q + dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x), mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

// crests are sharper than troughs, like real ripples
fn ridge(p: vec2<f32>) -> f32 {
    return 1.0 - abs(2.0 * noise(p) - 1.0);
}

fn height(p: vec2<f32>, t: f32) -> f32 {
    let f = w.flow.xy;
    let side = vec2<f32>(-f.y, f.x);
    let wind = 0.6 + w.sunc.w;
    // slow swell carried by the current, two crossing sets of ripples, fine chop
    var h = 0.50 * noise(p * 0.22 + f * t * 0.35);
    h += 0.30 * ridge(p * 0.42 + (f * 0.8 + side * 0.5) * t * 0.5);
    h += 0.16 * ridge(vec2<f32>(p.x * 0.8 - p.y * 0.6, p.x * 0.6 + p.y * 0.8) * 1.0 + (f * 0.6 - side * 0.7) * t * 0.8);
    h += 0.07 * wind * ridge(p * 3.1 - side * t * 1.3);
    // rain pocks the surface
    h += 0.10 * w.sky.w * noise(p * 7.0 + vec2<f32>(t * 5.0, -t * 4.0));
    return h;
}

@fragment
fn fragment(in: In) -> @location(0) vec4<f32> {
    let p = in.world.xz;
    let t = w.cam.w;
    let day = w.sun.w;

    let e = 0.12;
    let h0 = height(p, t);
    let hx = height(p + vec2<f32>(e, 0.0), t);
    let hz = height(p + vec2<f32>(0.0, e), t);
    let n = normalize(vec3<f32>(-(hx - h0) / e * 0.4, 1.0, -(hz - h0) / e * 0.4));

    let v = normalize(w.cam.xyz - in.world.xyz);
    let l = normalize(w.sun.xyz);
    let ndv = max(dot(n, v), 0.0);

    // water body: lighter in the shallows of the pattern and where a wave faces the light
    let patch_ = noise(p * 0.035 + w.flow.xy * t * 0.02);
    var body = w.deep.rgb * (0.75 + 0.5 * patch_) * (0.72 + 0.28 * h0);
    body = body * (0.3 + 0.7 * day) * (0.85 + 0.3 * max(dot(n, l), 0.0));
    // at night the river still catches the glow of the city
    body += vec3<f32>(0.012, 0.02, 0.04) * (1.0 - day) * (0.5 + h0);

    // sky mirrored in the surface, stronger at grazing angles
    let fres = 0.03 + 0.97 * pow(1.0 - ndv, 5.0);
    let r = reflect(-v, n);
    let sky = w.sky.rgb * (0.75 + 0.5 * (1.0 - clamp(r.y, 0.0, 1.0)));
    var col = mix(body, sky, clamp(fres * 1.6, 0.0, 1.0));

    // glitter: the light dancing on wave facets. (Looked at from above and from the south the
    // true mirror image of a southern sun never reaches the camera, so the glitter path is
    // taken from the light's mirror image across the view: what you would see looking sunwards.)
    let lg = normalize(vec3<f32>(l.x, l.y, -l.z));
    let s = max(dot(r, lg), 0.0);
    col += w.sunc.rgb * (pow(s, 220.0) * 7.0 + pow(s, 28.0) * 0.22);

    // streaks of foam and silt drawn out along the current
    let f = w.flow.xy;
    let along = dot(p, f);
    let across = dot(p, vec2<f32>(-f.y, f.x));
    let streak = noise(vec2<f32>(across * 0.9, along * 0.06 - t * 0.12));
    col += vec3<f32>(0.5, 0.55, 0.5) * smoothstep(0.78, 0.97, streak) * 0.12 * (0.2 + 0.8 * day);

    return vec4<f32>(col, 1.0);
}
