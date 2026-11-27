//! Linear color gradients across canvas cells.

use crate::canvas::Color;

/// The colors a gradient may produce, from most to least compatible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GradientPalette {
    /// The nearest of the 16 standard ANSI colors.
    Ansi,
    /// The nearest of the 256 indexed colors.
    Indexed,
    /// 24-bit RGB, one exact color per cell.
    Rgb,
}

impl GradientPalette {
    /// The palette a color was chosen from.
    pub fn of(color: Color) -> Self {
        match color {
            Color::Ansi(_) => Self::Ansi,
            Color::Indexed(_) => Self::Indexed,
            Color::Rgb(..) => Self::Rgb,
        }
    }

    /// The richest palette of the two endpoints, so a gradient between two
    /// 16-color endpoints stays within 16 colors.
    pub fn for_endpoints(start: Color, end: Color) -> Self {
        Self::of(start).max(Self::of(end))
    }
}

/// Returns the color of cell `point` in a linear gradient running from
/// `start` at cell `from` to `end` at cell `to`.
///
/// Each cell is projected onto the line from `from` to `to`, so the gradient
/// follows that direction whether it is horizontal, vertical, or diagonal.
/// Cells beyond either end keep that end's color; if `from == to`, every cell
/// gets `start`.
pub fn gradient_at(
    start: Color,
    end: Color,
    from: (usize, usize),
    to: (usize, usize),
    point: (usize, usize),
    palette: GradientPalette,
) -> Color {
    let delta = |a: usize, b: usize| b as i64 - a as i64;
    let (dx, dy) = (delta(from.0, to.0), delta(from.1, to.1));
    let length = dx * dx + dy * dy;
    let along = (delta(from.0, point.0) * dx + delta(from.1, point.1) * dy).clamp(0, length);
    let (a, b) = (start.rgb(), end.rgb());
    let rgb: [u8; 3] = std::array::from_fn(|i| {
        if length == 0 {
            return a[i];
        }
        let (a, b) = (i64::from(a[i]), i64::from(b[i]));
        // Rounded integer interpolation keeps both endpoints exact.
        let offset = (b - a) * along;
        (a + (offset + offset.signum() * length / 2) / length) as u8
    });
    quantize(rgb, palette)
}

fn quantize([r, g, b]: [u8; 3], palette: GradientPalette) -> Color {
    let nearest = |candidates: &mut dyn Iterator<Item = Color>| {
        candidates
            .min_by_key(|candidate| {
                let [cr, cg, cb] = candidate.rgb();
                [(r, cr), (g, cg), (b, cb)]
                    .iter()
                    .map(|&(x, y)| (i32::from(x) - i32::from(y)).pow(2))
                    .sum::<i32>()
            })
            .expect("palette is not empty")
    };
    match palette {
        GradientPalette::Rgb => Color::Rgb(r, g, b),
        GradientPalette::Indexed => nearest(&mut (0..=255).map(Color::Indexed)),
        GradientPalette::Ansi => nearest(&mut (0..16).map(Color::Ansi)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        start: Color,
        end: Color,
        from: usize,
        to: usize,
        palette: GradientPalette,
    ) -> Vec<Color> {
        (0..4)
            .map(|x| gradient_at(start, end, (from, 0), (to, 0), (x, 0), palette))
            .collect()
    }

    #[test]
    fn gradient_has_exact_endpoints_and_follows_the_line_direction() {
        let (red, blue) = (Color::Rgb(255, 0, 10), Color::Rgb(0, 255, 10));
        let steps = [
            Color::Rgb(255, 0, 10),
            Color::Rgb(170, 85, 10),
            Color::Rgb(85, 170, 10),
            Color::Rgb(0, 255, 10),
        ];
        assert_eq!(row(red, blue, 0, 3, GradientPalette::Rgb), steps);
        let mut reversed = steps;
        reversed.reverse();
        assert_eq!(row(red, blue, 3, 0, GradientPalette::Rgb), reversed);

        // Vertical lines give every cell in a row the same color.
        let vertical = |x, y| gradient_at(red, blue, (2, 0), (2, 3), (x, y), GradientPalette::Rgb);
        assert_eq!(vertical(0, 1), steps[1]);
        assert_eq!(vertical(3, 1), steps[1]);
        // Diagonal lines colour by distance along the line.
        let diagonal = |x, y| gradient_at(red, blue, (0, 0), (2, 2), (x, y), GradientPalette::Rgb);
        assert_eq!(diagonal(0, 0), red);
        assert_eq!(diagonal(2, 2), blue);
        assert_eq!(diagonal(2, 0), diagonal(1, 1));
        assert_eq!(diagonal(0, 2), diagonal(1, 1));

        // Cells beyond the ends clamp, and a zero-length line uses the start.
        assert_eq!(row(red, blue, 1, 2, GradientPalette::Rgb)[0], red);
        assert_eq!(row(red, blue, 1, 2, GradientPalette::Rgb)[3], blue);
        assert_eq!(row(red, blue, 1, 1, GradientPalette::Rgb), [red; 4]);
    }

    #[test]
    fn endpoints_choose_the_richest_palette() {
        let (ansi, indexed, rgb) = (Color::Ansi(1), Color::Indexed(1), Color::Rgb(1, 2, 3));
        assert_eq!(
            GradientPalette::for_endpoints(ansi, ansi),
            GradientPalette::Ansi
        );
        assert_eq!(
            GradientPalette::for_endpoints(indexed, ansi),
            GradientPalette::Indexed
        );
        assert_eq!(
            GradientPalette::for_endpoints(ansi, rgb),
            GradientPalette::Rgb
        );
        assert_eq!(
            GradientPalette::for_endpoints(rgb, indexed),
            GradientPalette::Rgb
        );
    }

    #[test]
    fn palette_gradients_use_the_nearest_supported_color() {
        let ansi = row(Color::Ansi(0), Color::Ansi(15), 0, 2, GradientPalette::Ansi);
        assert_eq!(ansi[..3], [Color::Ansi(0), Color::Ansi(7), Color::Ansi(15)]);
        let indexed = row(
            Color::Rgb(0, 0, 0),
            Color::Rgb(255, 0, 0),
            0,
            2,
            GradientPalette::Indexed,
        );
        assert_eq!(indexed[0].rgb(), [0, 0, 0]);
        assert_eq!(indexed[1], Color::Indexed(88));
        assert_eq!(indexed[2].rgb(), [255, 0, 0]);
        assert!(indexed
            .iter()
            .all(|color| matches!(color, Color::Indexed(_))));
    }
}
