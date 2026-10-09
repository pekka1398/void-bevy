// Transport existing f32 bits, never resample terrain or recompute f64 seams.
struct Params {
    n: u32, vertices: u32, input_start: u32, vertex_start: u32,
    index_start: u32, index_bits: u32, cell_bits: u32, padding: u32,
}
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> vertices: array<u32>;
@group(0) @binding(2) var<storage, read_write> indices: array<u32>;
#ifdef BATCHED_PACK
@group(0) @binding(3) var<storage, read> jobs: array<Params>;
#else
@group(0) @binding(3) var<uniform> single_params: Params;
#endif
@compute @workgroup_size(64)
fn pack(@builtin(global_invocation_id) id: vec3<u32>) {
#ifdef BATCHED_PACK
    let p = jobs[id.y];
#else
    let p = single_params;
#endif
    let v = id.x;
    if v < p.vertices {
        let src = p.input_start;
        let dst = p.vertex_start + v * 12u;
        for (var a = 0u; a < 3u; a++) {
            vertices[dst+a] = input[src+v*3u+a];
            vertices[dst+3u+a] = input[src+p.vertices*3u+v*3u+a];
            vertices[dst+6u+a] = input[src+p.vertices*6u+v*3u+a];
        }
        vertices[dst+9u] = 0x3f800000u;
        vertices[dst+10u] = input[src+p.vertices*9u+v];
        vertices[dst+11u] = p.cell_bits;
    }
    let cells = (p.n-1u)*(p.n-1u);
    if v < cells {
        let a = (v/(p.n-1u))*p.n+v%(p.n-1u);
        let b = a+1u;
        let c = a+p.n;
        let d = c+1u;
        if p.index_bits == 16u {
            let dst = p.index_start+v*3u;
            indices[dst] = a|(b<<16u);
            indices[dst+1u] = d|(a<<16u);
            indices[dst+2u] = d|(c<<16u);
        } else {
            let dst = p.index_start+v*6u;
            indices[dst]=a; indices[dst+1u]=b; indices[dst+2u]=d;
            indices[dst+3u]=a; indices[dst+4u]=d; indices[dst+5u]=c;
        }
    }
}
