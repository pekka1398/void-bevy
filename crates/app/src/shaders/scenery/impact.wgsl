#define_import_path void::impact
// The local impact population from void-terrain/src/impact/mod.rs. Mesh/collision remain the
// authoritative height field. This supplies only the analytic slope band missing from the mesh;
// as geometry refines, the mesh takes over. Integer identity, radius, wear and profile match Rust.
fn hash32(value: u32) -> u32 {
    var n = value ^ (value >> 16u);
    n *= 0x7feb352du;
    n ^= n >> 15u;
    n *= 0x846ca68bu;
    return n ^ (n >> 16u);
}
fn random32(value: u32) -> f32 { return f32(hash32(value) >> 8u) / 16777216.0; }
fn grad(cell: vec3<i32>, f: vec3<f32>) -> f32 {
    let c = vec3<u32>(cell);
    var h = (c.x * 374761393u) ^ (c.y * 668265263u) ^ (c.z * 1442695041u);
    h = (h ^ (h >> 13u)) * 1274126177u;
    h ^= h >> 16u;
    var g = array<vec3<f32>,12>(vec3(1.,1.,0.),vec3(-1.,1.,0.),vec3(1.,-1.,0.),vec3(-1.,-1.,0.),
        vec3(1.,0.,1.),vec3(-1.,0.,1.),vec3(1.,0.,-1.),vec3(-1.,0.,-1.),
        vec3(0.,1.,1.),vec3(0.,-1.,1.),vec3(0.,1.,-1.),vec3(0.,-1.,-1.));
    return dot(g[h % 12u],f);
}
fn impact_noise(d: vec3<f32>, frequency: f32, seed: f32) -> f32 {
    let p = d*frequency + vec3(seed,3.71,-5.13);
    let c = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f*f*f*(f*(f*6.0-15.0)+10.0);
    return mix(mix(mix(grad(c,f),grad(c+vec3(1,0,0),f-vec3(1.,0.,0.)),u.x),
                   mix(grad(c+vec3(0,1,0),f-vec3(0.,1.,0.)),grad(c+vec3(1,1,0),f-vec3(1.,1.,0.)),u.x),u.y),
               mix(mix(grad(c+vec3(0,0,1),f-vec3(0.,0.,1.)),grad(c+vec3(1,0,1),f-vec3(1.,0.,1.)),u.x),
                   mix(grad(c+vec3(0,1,1),f-vec3(0.,1.,1.)),grad(c+vec3(1,1,1),f-vec3(1.,1.,1.)),u.x),u.y),u.z);
}
fn impact_plains(d: vec3<f32>, seed: u32, amount: f32) -> f32 {
    let s = f32(seed % 10007u)*0.173;
    let warp = d+vec3(impact_noise(d,4.,s+71.),impact_noise(d,4.,s+19.),impact_noise(d,4.,s+39.))*0.07;
    return smoothstep(0.5-amount,0.68-amount,impact_noise(warp,5.3,s+31.));
}
fn bump_derivative(x: f32) -> f32 { return -4.0*x*max(1.0-x*x,0.0); }
fn smooth_derivative(a: f32,b: f32,x: f32) -> f32 {
    let t = clamp((x-a)/(b-a),0.0,1.0);
    return 6.0*t*(1.0-t)/(b-a);
}
fn bump(x: f32) -> f32 { let t=max(1.0-x*x,0.0); return t*t; }
fn crater_profile(x0: f32,width: f32,age: f32,complex: f32,preservation: f32) -> f32 {
    let x=abs(x0);
    return -(1.0-smoothstep(0.2+complex*(0.42+0.10*age),1.0,x)) + bump((x-1.0)/width)*(0.23+0.17*age)*preservation
        +bump(x/0.22)*complex*0.48+bump((x-0.76)/0.065)*complex*age*0.13;
}
struct ImpactDetail { slope: vec3<f32>, fresh: f32, dark: f32 }
fn impact_detail(d: vec3<f32>, radius: f32, cell: f32, pixel: f32, seed: u32, density: f32, plain: f32) -> ImpactDetail {
    var result = ImpactDetail(vec3(0.0),0.0,0.0);
    if density<=0.0 {return result;}
    var frequency = 6.0;
    for(var octave=0u;octave<9u;octave++) {
        let nominal = radius/frequency;
        if nominal*0.8 <= pixel*1.5 { break; }
        // No need to search a frequency whose smallest feature is fully represented by the mesh.
        {
            for(var face=0u;face<6u;face++) {
                var n=vec3(0.0); var u=vec3(0.0); var v=vec3(0.0);
                if face<2u {n=vec3(1.,0.,0.);u=vec3(0.,1.,0.);v=vec3(0.,0.,1.);}
                else if face<4u {n=vec3(0.,1.,0.);u=vec3(1.,0.,0.);v=vec3(0.,0.,1.);}
                else {n=vec3(0.,0.,1.);u=vec3(1.,0.,0.);v=vec3(0.,1.,0.);}
                if (face % 2u)==1u {n=-n;}
                let dn=dot(d,n);
                if dn<0.4 {continue;}
                let ij=vec2<i32>(floor(vec2(dot(d,u),dot(d,v))/dn*frequency));
                for(var oy=-1;oy<=1;oy++) {for(var ox=-1;ox<=1;ox++) {
                    let c=ij+vec2(ox,oy);
                    if abs(f32(c.x))>frequency+1.0 || abs(f32(c.y))>frequency+1.0 {continue;}
                    let id=hash32((u32(c.x)*374761393u)^(u32(c.y)*668265263u)^(face*2246822519u)^(octave*3266489917u)^seed);
                    if random32(id)>density {continue;}
                    let center=normalize(n+u*((f32(c.x)+random32(id+1u))/frequency)+v*((f32(c.y)+random32(id+2u))/frequency));
                    let cn=dot(center,n);
                    let cr=nominal*(0.07+0.26*pow(random32(id+3u),1.7))*cn*cn;
                    let mesh=smoothstep(cell*1.5,cell*3.0,cr*1.3);
                    let visible=smoothstep(pixel*1.5,pixel*3.0,cr*1.3)*smoothstep(1.0,3.0,cr);
                    if visible<=0.0 {continue;}
                    let delta=d-center;
                    let distance=length(delta);
                    let distance_ratio=distance*radius/cr;
                    if distance_ratio>=2.1 || distance<1e-9 {continue;}
                    let age=random32(id+4u);
                    let rough=impact_noise(d,radius/cr*5.0,f32(id%101u));
                    let distortion=1.0+(0.025+0.10*(1.0-age))*rough;
                    let x=distance_ratio*distortion;
                    let preservation=1.0-(1.0-age)*0.65*(0.5+0.5*rough);
                    let young=smoothstep(0.91,0.99,age);
                    let survival=1.0-plain*(1.0-young)*0.94;
                    let depth=min(cr*0.21,1800.0)*(0.28+0.72*age)*survival;
                    let width=0.11+0.17*(1.0-age);
                    let complex=smoothstep(6000.0,14000.0,cr);
                    let analytic=smooth_derivative(0.2+complex*(0.42+0.10*age),1.0,x)
                        +bump_derivative((x-1.0)/width)/width*(0.23+0.17*age)*preservation
                        +bump_derivative(x/0.22)/0.22*complex*0.48
                        +bump_derivative((x-0.76)/0.065)/0.065*complex*age*0.13;
                    // Tile normals are central differences over a cell, not the exact derivative
                    // of the band-limited height. Subtract that resolved slope before adding the
                    // pixel footprint's slope, including narrow rims on otherwise resolved craters.
                    let ds=max(pixel/cr*distortion,0.002);
                    let ms=max(cell/cr*distortion,0.002);
                    var fine=analytic;
                    if ds>0.02 {fine=(crater_profile(x+ds,width,age,complex,preservation)-crater_profile(x-ds,width,age,complex,preservation))/(2.0*ds);}
                    var coarse=analytic;
                    if ms>0.02 {coarse=(crater_profile(x+ms,width,age,complex,preservation)-crater_profile(x-ms,width,age,complex,preservation))/(2.0*ms);}
                    let slope=(fine*visible-coarse*mesh)*distortion;
                    let along=delta/distance;
                    result.slope += (along-d*dot(d,along))*(depth/cr*slope);
                    let ejecta=bump((x-1.25)/0.85)*(0.25+0.75*young)*(1.0-smoothstep(1.8,2.1,distance_ratio));
                    let fresh=(bump((x-1.0)/width)*0.35+ejecta*0.65)*young*survival*visible;
                    result.fresh=max(result.fresh,fresh);
                    result.dark=max(result.dark,(1.0-smoothstep(0.2+complex*(0.42+0.10*age),1.0,x))*(1.0-young)*0.17*survival*visible);
                }}
            }
        }
        frequency*=3.0;
    }
    return result;
}

fn rayed_color(d: vec3<f32>, center: vec3<f32>, size: f32, seed: f32, freshness: f32) -> f32 {
    let x=length(d-center)/size;
    if x>=15.0 {return 0.0;}
    let q=x*(1.0+0.025*impact_noise(d,7.0/size,seed));
    if x<0.9 {return (bump((x-1.18)/1.3)*0.37+bump((q-1.0)/0.12)*0.30)*freshness;}
    let axis=select(vec3(1.,0.,0.),vec3(0.,0.,1.),abs(center.z)<0.9);
    let t=normalize(cross(center,axis));
    let b=cross(center,t);
    let angle=atan2(dot(d,b),dot(d,t))+(0.07+x*0.005)*impact_noise(d,93.,seed+11.)+0.035*log(1.0+x);
    let lobes=sin(angle*11.0+seed)*0.48+sin(angle*19.0-seed*0.31)*0.32+sin(angle*31.0+1.7)*0.20;
    let streak=smoothstep(0.23,0.67,lobes);
    let breakup=0.58+0.42*impact_noise(d,137.,seed+3.);
    let ray=streak*breakup*(1.0-smoothstep(2.0,15.0,x))*smoothstep(0.9,1.6,x)/(1.0+x*0.12);
    return (ray*1.6+bump((x-1.18)/1.3)*0.37+bump((q-1.0)/0.12)*0.30)*freshness;
}
fn regional_albedo(d: vec3<f32>, seed: u32, plain: f32, mature: vec3<f32>, plains: vec3<f32>) -> vec4<f32> {
    let s=f32(seed%10007u)*0.173;
    let region=impact_noise(d,3.1,s);
    let warp=d+vec3(impact_noise(d,4.,s+71.),impact_noise(d,4.,s+19.),impact_noise(d,4.,s+39.))*0.07;
    let unit=smoothstep(-0.18,0.22,impact_noise(warp,7.2,s+177.));
    let mottling=(0.78+0.26*unit)*(1.0+0.16*region+0.13*impact_noise(d,37.,s+117.));
    return vec4(mix(mature,plains,plain*0.65)*mottling,mottling);
}
