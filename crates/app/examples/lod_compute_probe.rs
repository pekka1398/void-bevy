//! Isolated GPU geometry packing experiment; does not change the game renderer.
//! Run: cargo run -j 2 -p void-app --example lod_compute_probe --release -- [n=33] [tiles=64] [repeats=30] [warmup=5]
//! Uses synthetic f32 attributes, real Bevy Mesh packing, and actual grid topology.
//! Terrain sampling, stitching, allocator growth and scene rendering are excluded.
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, Mesh, MeshVertexAttribute},
    render::render_resource::{PrimitiveTopology, VertexFormat},
    tasks::block_on,
};
use std::{borrow::Cow, sync::mpsc, time::Instant};
use wgpu::util::DeviceExt;

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|v| v.to_ne_bytes()).collect()
}
fn words(data: &[u8]) -> Vec<u32> {
    let (words, remainder) = data.as_chunks::<4>();
    assert!(remainder.is_empty(), "GPU payload is not word-aligned");
    words.iter().map(|v| u32::from_ne_bytes(*v)).collect()
}
fn read(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u8> {
    let (tx, rx) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU readback poll");
    rx.recv().unwrap().expect("GPU readback mapping");
    let data = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    data
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let arg = |i: usize, default: usize| {
        args.get(i)
            .map(|s| s.parse().expect("integer argument"))
            .unwrap_or(default)
    };
    let (n, tiles, repeats, warmup) = (arg(0, 33), arg(1, 64), arg(2, 30), arg(3, 5));
    assert!((2..=256).contains(&n) && tiles > 0 && repeats > 0);
    let vertices = n * n;
    let index_count = 6 * (n - 1) * (n - 1);
    let mut meshes = Vec::with_capacity(tiles);
    let mut soa = Vec::with_capacity(vertices * tiles * 11);
    let mut expected_indices = Vec::with_capacity(index_count * tiles);
    let mut make_indices_ms = 0.0;
    for tile in 0..tiles {
        let values: Vec<u32> = (0..vertices * 11)
            .map(|i| {
                // Explicit payloads exercise transport bit equality, not floating arithmetic.
                match i % 257 {
                    0 => 0x80000000,
                    1 => 0x7fc12345,
                    2 => 0x7fa54321,
                    _ => ((i + tile * vertices) as f32 * 0.125).to_bits(),
                }
            })
            .collect();
        let triples = |offset: usize| {
            values[offset..offset + vertices * 3]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|v| {
                    [
                        f32::from_bits(v[0]),
                        f32::from_bits(v[1]),
                        f32::from_bits(v[2]),
                    ]
                })
                .collect::<Vec<_>>()
        };
        let colors = triples(vertices * 6)
            .into_iter()
            .map(|c| [c[0], c[1], c[2], 1.0])
            .collect::<Vec<_>>();
        let scalar = |offset: usize| {
            values[offset..offset + vertices]
                .iter()
                .map(|v| f32::from_bits(*v))
                .collect::<Vec<_>>()
        };
        let start = Instant::now();
        let (mut indices, grid_count) = void_lod::build_tile_indices(n);
        indices.truncate(grid_count);
        make_indices_ms += start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(indices.len(), index_count);
        expected_indices.extend_from_slice(&indices);
        meshes.push(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, triples(0))
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, triples(vertices * 3))
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_attribute(
                MeshVertexAttribute::new("Height", 917_330_201, VertexFormat::Float32),
                scalar(vertices * 9),
            )
            .with_inserted_attribute(
                MeshVertexAttribute::new("TerrainCell", 917_330_202, VertexFormat::Float32),
                scalar(vertices * 10),
            )
            .with_inserted_indices(Indices::U32(indices)),
        );
        soa.extend(values);
    }
    // Match Bevy's attribute ordering and actual interleaving implementation.
    let mut cpu_pack = Vec::new();
    let mut expected = Vec::new();
    for iteration in 0..warmup + repeats {
        let start = Instant::now();
        let packed: Vec<Vec<u8>> = meshes
            .iter()
            .map(Mesh::create_packed_vertex_buffer_data)
            .collect();
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if iteration >= warmup {
            cpu_pack.push(elapsed);
        }
        if iteration == 0 {
            expected.extend(packed.into_iter().flat_map(|v| words(&v)));
            expected.extend_from_slice(&expected_indices);
        }
    }
    let upload = bytes(&soa);
    let output_bytes = (expected.len() * 4) as u64;
    let init = Instant::now();
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("GPU adapter unavailable; no CPU replacement for this experiment");
    let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("LOD packing probe"),
        required_features: if timestamps {
            wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .expect("GPU device request");
    let device_init_ms = init.elapsed().as_secs_f64() * 1000.0;
    let input = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("input SoA"),
        size: upload.len() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("GPU resident mesh"),
        size: output_bytes,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::VERTEX
            | wgpu::BufferUsages::INDEX,
        mapped_at_creation: false,
    });
    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("grid params"),
        contents: &bytes(&[n as u32, tiles as u32, vertices as u32, index_count as u32]),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("validation readback"),
        size: output_bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("LOD bit-preserving packing"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("lod_compute_probe.wgsl"))),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = |entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let pack = pipeline("pack");
    let indices = pipeline("indices");
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
        ],
    });
    let queries = timestamps.then(|| {
        device.create_query_set(&wgpu::QuerySetDescriptor {
            label: None,
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        })
    });
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let query_readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    println!(
        "{}",
        serde_json::json!({"probe":"lod_compute_pack", "adapter":format!("{:?}",adapter.get_info()), "n":n,"tiles":tiles,"warmup":warmup,"repeats":repeats,"device_init_ms_excluded":device_init_ms,"timestamp_supported":timestamps,"input_bytes":upload.len(),"output_bytes":output_bytes,"cpu_mesh_upload_bytes":output_bytes,"cpu_resident_packing":"write_packed_vertex_buffer_data into preallocated buffers; input SoA gathering and allocator growth excluded from both modes","cpu_initial_indices_ms":make_indices_ms,"cpu_mesh_create_packed_vertex_buffer_data_ms":cpu_pack})
    );
    let index_upload = bytes(&expected_indices);
    // Match the allocator's packing API with staging storage already allocated.
    let mut packed: Vec<Vec<u8>> = meshes
        .iter()
        .map(|mesh| vec![0; mesh.get_vertex_buffer_size()])
        .collect();
    for iteration in 0..warmup + repeats {
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let start = Instant::now();
        for (mesh, buffer) in meshes.iter().zip(&mut packed) {
            mesh.write_packed_vertex_buffer_data(wgpu::WriteOnly::from_mut(buffer));
        }
        let pack_ms = start.elapsed().as_secs_f64() * 1000.0;
        let write = Instant::now();
        let mut offset = 0;
        for tile in &packed {
            queue.write_buffer(&output, offset, tile);
            offset += tile.len() as u64;
        }
        queue.write_buffer(&output, offset, &index_upload);
        let queue_write_ms = write.elapsed().as_secs_f64() * 1000.0;
        let submit = Instant::now();
        queue.submit([]);
        let submit_ms = submit.elapsed().as_secs_f64() * 1000.0;
        let wait = Instant::now();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let completion_ms = wait.elapsed().as_secs_f64() * 1000.0;
        if iteration >= warmup {
            println!(
                "{}",
                serde_json::json!({"mode":"cpu_mesh_pack_and_upload_resident", "iteration":iteration-warmup, "pack_ms":pack_ms, "queue_write_ms":queue_write_ms, "submit_ms":submit_ms, "completion_wait_ms":completion_ms, "total_ms":start.elapsed().as_secs_f64()*1000.0, "upload_bytes":output_bytes, "queue_write_calls":tiles+1})
            );
        }
    }
    // Exact allocator-style direct staging: one vertex and one index write per mesh.
    // No intermediate packed Vec or extra vertex memcpy is needed by this path.
    for iteration in 0..warmup + repeats {
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let start = Instant::now();
        let mut offset = 0;
        for mesh in &meshes {
            let size = mesh.get_vertex_buffer_size() as u64;
            {
                let mut staging_view = queue
                    .write_buffer_with(&output, offset, std::num::NonZeroU64::new(size).unwrap())
                    .expect("allocator staging allocation");
                mesh.write_packed_vertex_buffer_data(staging_view.slice(..));
            }
            offset += size;
        }
        for chunk in index_upload.chunks_exact(index_count * 4) {
            {
                let mut staging_view = queue
                    .write_buffer_with(
                        &output,
                        offset,
                        std::num::NonZeroU64::new(chunk.len() as u64).unwrap(),
                    )
                    .expect("index staging allocation");
                staging_view.copy_from_slice(chunk);
            }
            offset += chunk.len() as u64;
        }
        let pack_and_stage_ms = start.elapsed().as_secs_f64() * 1000.0;
        let submit = Instant::now();
        queue.submit([]);
        let submit_ms = submit.elapsed().as_secs_f64() * 1000.0;
        let wait = Instant::now();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let completion_ms = wait.elapsed().as_secs_f64() * 1000.0;
        if iteration >= warmup {
            println!(
                "{}",
                serde_json::json!({"mode":"cpu_allocator_direct_staging","iteration":iteration-warmup,"pack_and_stage_ms":pack_and_stage_ms,"submit_ms":submit_ms,"completion_wait_ms":completion_ms,"total_ms":start.elapsed().as_secs_f64()*1000.0,"upload_bytes":output_bytes,"queue_write_calls":tiles*2})
            );
        }
    }
    for readback in [false, true] {
        for iteration in 0..warmup + repeats {
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let start = Instant::now();
            queue.write_buffer(&input, 0, &upload);
            let upload_ms = start.elapsed().as_secs_f64() * 1000.0;
            let encode = Instant::now();
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("pack and indices"),
                    timestamp_writes: queries.as_ref().map(|query_set| {
                        wgpu::ComputePassTimestampWrites {
                            query_set,
                            beginning_of_pass_write_index: Some(0),
                            end_of_pass_write_index: Some(1),
                        }
                    }),
                });
                pass.set_bind_group(0, &group, &[]);
                pass.set_pipeline(&pack);
                pass.dispatch_workgroups((vertices * tiles).div_ceil(64) as u32, 1, 1);
                pass.set_pipeline(&indices);
                pass.dispatch_workgroups(((n - 1) * (n - 1) * tiles).div_ceil(64) as u32, 1, 1);
            }
            if readback {
                encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, output_bytes);
            }
            let encode_ms = encode.elapsed().as_secs_f64() * 1000.0;
            let submit = Instant::now();
            queue.submit([encoder.finish()]);
            let submit_ms = submit.elapsed().as_secs_f64() * 1000.0;
            let wait = Instant::now();
            let result = if readback {
                Some(read(&device, &staging))
            } else {
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                None
            };
            let completion_ms = wait.elapsed().as_secs_f64() * 1000.0;
            let total_ms = start.elapsed().as_secs_f64() * 1000.0;
            let validation = Instant::now();
            if let Some(data) = result {
                assert_eq!(words(&data), expected, "GPU mesh payload/topology mismatch");
            }
            let validation_ms = validation.elapsed().as_secs_f64() * 1000.0;
            // Separate query resolution/readback from the measured resident submission.
            let kernel_ms = queries.as_ref().map(|q| {
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                encoder.resolve_query_set(q, 0..2, &resolve, 0);
                encoder.copy_buffer_to_buffer(&resolve, 0, &query_readback, 0, 16);
                queue.submit([encoder.finish()]);
                let data = read(&device, &query_readback);
                let a = u64::from_ne_bytes(data[0..8].try_into().unwrap());
                let b = u64::from_ne_bytes(data[8..16].try_into().unwrap());
                b.wrapping_sub(a) as f64 * f64::from(queue.get_timestamp_period()) / 1e6
            });
            if iteration >= warmup {
                println!(
                    "{}",
                    serde_json::json!({"mode":if readback {"end_to_end_readback"} else {"gpu_resident_no_readback"},"iteration":iteration-warmup,"queue_write_ms":upload_ms,"encode_ms":encode_ms,"submit_ms":submit_ms,"completion_wait_ms":completion_ms,"total_ms":total_ms,"kernel_ms":kernel_ms,"bitwise_verified":readback,"validation_ms_excluded":validation_ms})
                );
            }
        }
    }
}
