// Integer loads/stores preserve every f32 payload, including NaNs and signed zero.
struct Params { n: u32, tiles: u32, vertices: u32, indices: u32 }
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(64)
fn pack(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    if k >= params.vertices * params.tiles { return; }
    // Input is SoA per tile: position3, normal3, color3, height1, cell1.
    let tile = k / params.vertices;
    let v = k % params.vertices;
    let base = tile * params.vertices * 11u;
    let dst = k * 12u;
    for (var a = 0u; a < 3u; a++) {
        output[dst + a] = input[base + v * 3u + a];
        output[dst + 3u + a] = input[base + params.vertices * 3u + v * 3u + a];
        output[dst + 6u + a] = input[base + params.vertices * 6u + v * 3u + a];
    }
    output[dst + 9u] = 0x3f800000u;
    output[dst + 10u] = input[base + params.vertices * 9u + v];
    output[dst + 11u] = input[base + params.vertices * 10u + v];
}

@compute @workgroup_size(64)
fn indices(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x;
    let cells = (params.n - 1u) * (params.n - 1u);
    if k >= cells * params.tiles { return; }
    let tile = k / cells;
    let cell = k % cells;
    let a = (cell / (params.n - 1u)) * params.n + cell % (params.n - 1u);
    let b = a + 1u;
    let c = a + params.n;
    let d = c + 1u;
    let dst = params.vertices * params.tiles * 12u + tile * params.indices + cell * 6u;
    output[dst] = a; output[dst + 1u] = b; output[dst + 2u] = d;
    output[dst + 3u] = a; output[dst + 4u] = d; output[dst + 5u] = c;
}
