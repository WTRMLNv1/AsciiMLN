use crate::framebuffer::{Framebuffer, TerminalColor};
use crate::math::vec::Vec3;

#[derive(Debug, Clone, Copy)]
pub struct ScreenVertex {
    pub x: f32,
    pub y: f32,
    pub depth: f32,
}

#[derive(Debug, Clone, Copy)]
struct ClipVertex {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

pub fn draw_line(fb: &mut Framebuffer, p1: (f32, f32), p2: (f32, f32), ch: char) {
    let (x1, y1) = (p1.0 as i32, p1.1 as i32);
    let (x2, y2) = (p2.0 as i32, p2.1 as i32);

    let dx = (x2 - x1).abs();
    let dy = -(y2 - y1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let sy = if y1 < y2 { 1 } else { -1 };
    let mut err = dx + dy;

    let mut curr_x = x1;
    let mut curr_y = y1;

    loop {
        fb.set(curr_x as usize, curr_y as usize, ch);
        if curr_x == x2 && curr_y == y2 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            curr_x += sx;
        }
        if e2 <= dx {
            err += dx;
            curr_y += sy;
        }
    }
}

pub fn shade_char(brightness: f32) -> char {
    const GRADIENT: &[u8] = b".:-=+*#%@";
    let clamped = brightness.clamp(0.0, 1.0);
    let idx = (clamped * (GRADIENT.len() - 1) as f32).round() as usize;
    GRADIENT[idx] as char
}

pub fn project_vertex_with_depth(
    v: Vec3,
    mvp: &crate::math::mat::Mat4,
    width: usize,
    height: usize,
) -> Option<ScreenVertex> {
    let clip = mvp.transform_vec(v);
    project_clip_vertex(
        ClipVertex {
            x: clip[0],
            y: clip[1],
            z: clip[2],
            w: clip[3],
        },
        width,
        height,
    )
}

/// Clip a world-space triangle against all six homogeneous view-frustum planes,
/// then project the remaining polygon into rasterizable triangles.  Clipping
/// before the perspective divide avoids the huge coordinates that arise when a
/// face crosses the near plane or the edge of the screen.
pub fn clip_and_project_triangle_with_depth(
    vertices: [Vec3; 3],
    mvp: &crate::math::mat::Mat4,
    width: usize,
    height: usize,
) -> Vec<[ScreenVertex; 3]> {
    let clip_vertices = vertices.map(|vertex| {
        let clip = mvp.transform_vec(vertex);
        ClipVertex {
            x: clip[0],
            y: clip[1],
            z: clip[2],
            w: clip[3],
        }
    });
    let clipped = clip_triangle(clip_vertices);
    if clipped.len() < 3 {
        return Vec::new();
    }

    let projected: Vec<_> = clipped
        .into_iter()
        .map(|vertex| project_clip_vertex(vertex, width, height))
        .collect::<Option<_>>()
        .unwrap_or_default();
    if projected.len() < 3 {
        return Vec::new();
    }

    (1..projected.len() - 1)
        .map(|index| [projected[0], projected[index], projected[index + 1]])
        .collect()
}

pub fn rasterize_triangle(
    fb: &mut Framebuffer,
    a: ScreenVertex,
    b: ScreenVertex,
    c: ScreenVertex,
    ch: char,
    color: TerminalColor,
) -> usize {
    let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as i32;
    let max_x = a.x.max(b.x).max(c.x).ceil().min((fb.width - 1) as f32) as i32;
    let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as i32;
    let max_y = a.y.max(b.y).max(c.y).ceil().min((fb.height - 1) as f32) as i32;

    let area = edge_function(a.x, a.y, b.x, b.y, c.x, c.y);
    if area.abs() < f32::EPSILON {
        return 0;
    }

    let mut written = 0;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let w0 = edge_function(b.x, b.y, c.x, c.y, px, py);
            let w1 = edge_function(c.x, c.y, a.x, a.y, px, py);
            let w2 = edge_function(a.x, a.y, b.x, b.y, px, py);

            let inside = if area > 0.0 {
                w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
            } else {
                w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
            };

            if inside {
                let alpha = w0 / area;
                let beta = w1 / area;
                let gamma = w2 / area;
                let depth = alpha * a.depth + beta * b.depth + gamma * c.depth;

                if fb.set_with_depth(x as usize, y as usize, depth, ch, color) {
                    written += 1;
                }
            }
        }
    }

    written
}

pub fn rasterize_transparent_triangle(
    fb: &mut Framebuffer,
    a: ScreenVertex,
    b: ScreenVertex,
    c: ScreenVertex,
    ch: char,
    color: TerminalColor,
    opacity: f32,
) -> usize {
    let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as i32;
    let max_x = a.x.max(b.x).max(c.x).ceil().min((fb.width - 1) as f32) as i32;
    let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as i32;
    let max_y = a.y.max(b.y).max(c.y).ceil().min((fb.height - 1) as f32) as i32;
    let area = edge_function(a.x, a.y, b.x, b.y, c.x, c.y);
    if area.abs() < f32::EPSILON {
        return 0;
    }

    let mut written = 0;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge_function(b.x, b.y, c.x, c.y, px, py);
            let w1 = edge_function(c.x, c.y, a.x, a.y, px, py);
            let w2 = edge_function(a.x, a.y, b.x, b.y, px, py);
            let inside = if area > 0.0 {
                w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
            } else {
                w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
            };
            if inside {
                let depth = (w0 * a.depth + w1 * b.depth + w2 * c.depth) / area;
                if fb
                    .blend_transparent_with_depth(x as usize, y as usize, depth, ch, color, opacity)
                {
                    written += 1;
                }
            }
        }
    }
    written
}

fn edge_function(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

pub fn project_vertex(
    v: Vec3,
    mvp: &crate::math::mat::Mat4,
    width: usize,
    height: usize,
) -> Option<(f32, f32)> {
    let clip = mvp.transform_vec(v);
    let projected = project_clip_vertex(
        ClipVertex {
            x: clip[0],
            y: clip[1],
            z: clip[2],
            w: clip[3],
        },
        width,
        height,
    )?;
    Some((projected.x, projected.y))
}

fn project_clip_vertex(vertex: ClipVertex, width: usize, height: usize) -> Option<ScreenVertex> {
    if vertex.w <= 0.0 {
        return None;
    }

    let ndc_x = vertex.x / vertex.w;
    let ndc_y = vertex.y / vertex.w;
    let ndc_z = vertex.z / vertex.w;
    if !(-1.0..=1.0).contains(&ndc_x)
        || !(-1.0..=1.0).contains(&ndc_y)
        || !(-1.0..=1.0).contains(&ndc_z)
    {
        return None;
    }

    Some(ScreenVertex {
        x: (ndc_x + 1.0) * 0.5 * width as f32,
        y: (1.0 - ndc_y) * 0.5 * height as f32,
        depth: (ndc_z + 1.0) * 0.5,
    })
}

fn clip_triangle(vertices: [ClipVertex; 3]) -> Vec<ClipVertex> {
    let mut polygon = vertices.to_vec();
    // The six clip-space inequalities are x/y/z >= -w and x/y/z <= w.
    for plane in 0..6 {
        polygon = clip_polygon_against_plane(&polygon, plane);
        if polygon.is_empty() {
            break;
        }
    }
    polygon
}

fn clip_polygon_against_plane(polygon: &[ClipVertex], plane: usize) -> Vec<ClipVertex> {
    if polygon.is_empty() {
        return Vec::new();
    }

    let mut clipped = Vec::with_capacity(polygon.len() + 1);
    let mut previous = *polygon.last().expect("polygon is known to be non-empty");
    let mut previous_distance = clip_plane_distance(previous, plane);
    for &current in polygon {
        let current_distance = clip_plane_distance(current, plane);
        let previous_inside = previous_distance >= 0.0;
        let current_inside = current_distance >= 0.0;

        if previous_inside != current_inside {
            let t = previous_distance / (previous_distance - current_distance);
            clipped.push(interpolate_clip_vertex(previous, current, t));
        }
        if current_inside {
            clipped.push(current);
        }

        previous = current;
        previous_distance = current_distance;
    }
    clipped
}

fn clip_plane_distance(vertex: ClipVertex, plane: usize) -> f32 {
    match plane {
        0 => vertex.x + vertex.w,
        1 => vertex.w - vertex.x,
        2 => vertex.y + vertex.w,
        3 => vertex.w - vertex.y,
        4 => vertex.z + vertex.w,
        5 => vertex.w - vertex.z,
        _ => unreachable!("only the six view-frustum planes are valid"),
    }
}

fn interpolate_clip_vertex(a: ClipVertex, b: ClipVertex, t: f32) -> ClipVertex {
    ClipVertex {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        z: a.z + (b.z - a.z) * t,
        w: a.w + (b.w - a.w) * t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::mat::Mat4;

    #[test]
    fn clipping_keeps_the_visible_portion_of_a_triangle() {
        let triangles = clip_and_project_triangle_with_depth(
            [
                Vec3::new(-2.0, 0.0, 0.0),
                Vec3::new(0.5, 0.8, 0.0),
                Vec3::new(0.5, -0.8, 0.0),
            ],
            &Mat4::identity(),
            100,
            50,
        );

        assert!(!triangles.is_empty());
        for triangle in triangles {
            for vertex in triangle {
                assert!((0.0..=100.0).contains(&vertex.x));
                assert!((0.0..=50.0).contains(&vertex.y));
            }
        }
    }

    #[test]
    fn single_vertex_projection_rejects_side_planes() {
        assert!(project_vertex(Vec3::new(1.1, 0.0, 0.0), &Mat4::identity(), 80, 24).is_none());
        assert!(project_vertex(Vec3::new(0.0, -1.1, 0.0), &Mat4::identity(), 80, 24).is_none());
    }
}
