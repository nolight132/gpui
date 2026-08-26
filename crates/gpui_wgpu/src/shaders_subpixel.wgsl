// --- subpixel sprites --- //

struct SubpixelSprite {
    order: u32,
    pad: u32,
    bounds: Bounds,
    content_mask: Bounds,
    color: Hsla,
    active_color: Hsla,
    sweep_front: f32,
    sweep_softness: f32,
    sweep_embolden: f32,
    sweep_progress: f32,
    tile: AtlasTile,
    transformation: TransformationMatrix,
}
@group(1) @binding(0) var<storage, read> b_subpixel_sprites: array<SubpixelSprite>;

struct SubpixelSpriteOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tile_position: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(2) @interpolate(flat) active_color: vec4<f32>,
    @location(3) clip_distances: vec4<f32>,
    @location(4) @interpolate(flat) sweep: vec4<f32>,
    @location(5) @interpolate(flat) tile_bounds: vec4<f32>,
}

struct SubpixelSpriteFragmentOutput {
    @location(0) @blend_src(0) foreground: vec4<f32>,
    @location(0) @blend_src(1) alpha: vec4<f32>,
}

@vertex
fn vs_subpixel_sprite(@builtin(vertex_index) vertex_id: u32, @builtin(instance_index) instance_id: u32) -> SubpixelSpriteOutput {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let sprite = b_subpixel_sprites[instance_id];

    var out = SubpixelSpriteOutput();
    out.position = to_device_position_transformed(unit_vertex, sprite.bounds, sprite.transformation);
    let atlas_size = vec2<f32>(textureDimensions(t_sprite, 0));
    let tile_origin = vec2<f32>(sprite.tile.bounds.origin);
    let tile_size = vec2<f32>(sprite.tile.bounds.size);
    let horizontal_padding = select(0.0, ceil(max(sprite.sweep_embolden, 0.0)), sprite.sweep_progress >= 0.0);
    let padded_origin = tile_origin - vec2<f32>(horizontal_padding, 0.0);
    let padded_size = tile_size + vec2<f32>(2.0 * horizontal_padding, 0.0);
    out.tile_position = (padded_origin + unit_vertex * padded_size) / atlas_size;
    out.color = hsla_to_rgba(sprite.color);
    out.active_color = hsla_to_rgba(sprite.active_color);
    out.clip_distances = distance_from_clip_rect_transformed(unit_vertex, sprite.bounds, sprite.content_mask, sprite.transformation);
    out.sweep = vec4<f32>(sprite.sweep_front, sprite.sweep_softness, sprite.sweep_embolden, sprite.sweep_progress);
    out.tile_bounds = vec4<f32>(tile_origin / atlas_size, (tile_origin + tile_size) / atlas_size);
    return out;
}

fn sample_subpixel_tile(position: vec2<f32>, tile_bounds: vec4<f32>) -> vec3<f32> {
    let inside = all(position >= tile_bounds.xy) && all(position <= tile_bounds.zw);
    let half_texel = vec2<f32>(0.5) / vec2<f32>(textureDimensions(t_sprite, 0));
    let safe_position = clamp(position, tile_bounds.xy + half_texel, tile_bounds.zw - half_texel);
    return select(vec3<f32>(0.0), textureSample(t_sprite, s_sprite, safe_position).rgb, inside);
}

fn shift_subpixels_left(previous: vec3<f32>, current: vec3<f32>, distance: f32) -> vec3<f32> {
    let one = vec3<f32>(previous.b, current.r, current.g);
    let two = vec3<f32>(previous.g, previous.b, current.r);
    if (distance < 1.0) {
        return mix(current, one, distance);
    }
    if (distance < 2.0) {
        return mix(one, two, distance - 1.0);
    }
    return mix(two, previous, distance - 2.0);
}

fn shift_subpixels_right(current: vec3<f32>, next: vec3<f32>, distance: f32) -> vec3<f32> {
    let one = vec3<f32>(current.g, current.b, next.r);
    let two = vec3<f32>(current.b, next.r, next.g);
    if (distance < 1.0) {
        return mix(current, one, distance);
    }
    if (distance < 2.0) {
        return mix(one, two, distance - 1.0);
    }
    return mix(two, next, distance - 2.0);
}

@fragment
fn fs_subpixel_sprite(input: SubpixelSpriteOutput) -> SubpixelSpriteFragmentOutput {
    var sample: vec3<f32>;
    var color = input.color;
    if (input.sweep.w < 0.0) {
        sample = textureSample(t_sprite, s_sprite, input.tile_position).rgb;
    } else {
        sample = sample_subpixel_tile(input.tile_position, input.tile_bounds);
        var transition = 0.0;
        if (input.sweep.w >= 1.0) {
            transition = 1.0;
        } else if (input.sweep.w > 0.0) {
            transition = 1.0 - smoothstep(input.sweep.x - input.sweep.y, input.sweep.x, input.position.x);
        }
        let pixel_step = dpdx(input.tile_position);
        let whole_pixels = floor(input.sweep.z);
        let subpixels = fract(input.sweep.z) * 3.0;
        let left_position = input.tile_position - pixel_step * whole_pixels;
        let right_position = input.tile_position + pixel_step * whole_pixels;
        var left_current = sample;
        var right_current = sample;
        if (whole_pixels > 0.0) {
            left_current = sample_subpixel_tile(left_position, input.tile_bounds);
            right_current = sample_subpixel_tile(right_position, input.tile_bounds);
        }
        let left_previous = sample_subpixel_tile(left_position - pixel_step, input.tile_bounds);
        let right_next = sample_subpixel_tile(right_position + pixel_step, input.tile_bounds);
        let left = shift_subpixels_left(left_previous, left_current, subpixels);
        let right = shift_subpixels_right(right_current, right_next, subpixels);
        sample = mix(sample, max(sample, max(left, right)), transition);
        color = mix(input.color, input.active_color, transition);
    }
    if (gamma_params.is_bgr != 0u) {
        sample = sample.bgr;
    }
    let alpha_corrected = apply_contrast_and_gamma_correction3(sample, color.rgb, gamma_params.subpixel_enhanced_contrast, gamma_params.gamma_ratios);

    // Alpha clip after using the derivatives.
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return SubpixelSpriteFragmentOutput(vec4<f32>(0.0), vec4<f32>(0.0));
    }

    var out = SubpixelSpriteFragmentOutput();
    out.foreground = vec4<f32>(color.rgb, 1.0);
    out.alpha = vec4<f32>(color.a * alpha_corrected, 1.0);
    return out;
}
