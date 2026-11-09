//! The avatar palette behind profile banners without their own color: `u` in module 515718 gathers every tenth pixel
//! that is opaque enough and not near white, and module 531325 (the `quantize` MMCQ library) cuts the color space into
//! boxes. Ported step for step, quirks included, because the first box's average is the banner color users see.

const SIGBITS: u32 = 5;
const RSHIFT: u32 = 8 - SIGBITS;
const MAX_ITERATIONS: usize = 1000;
const FRACT_BY_POPULATIONS: f64 = 0.75;

/// Module 998304, `Q7`: three hue rotations after lifting muted saturation and extreme lightness.
pub fn complementary_colors(rgb: [u8; 3]) -> [[u8; 3]; 3] {
    let [r, g, b] = rgb.map(|value| f64::from(value) / 255.);
    let low = r.min(g).min(b);
    let high = r.max(g).max(b);
    let delta = high - low;
    let hue = if delta == 0. {
        0.
    } else if high == r {
        ((g - b) / delta) % 6.
    } else if high == g {
        (b - r) / delta + 2.
    } else {
        (r - g) / delta + 4.
    };
    let mut hue = (hue * 60. + 0.5).floor().rem_euclid(360.);
    let lightness = (high + low) / 2.;
    let saturation = if delta == 0. {
        0.
    } else {
        delta / (1. - (2. * lightness - 1.).abs())
    };
    let mut saturation = (saturation * 1000.).round() / 10.;
    let mut lightness = (lightness * 1000.).round() / 10.;
    if saturation < 30. {
        saturation += 30.;
    }
    if lightness > 80. {
        lightness -= 40.;
    }
    if lightness < 20. {
        lightness += 15.;
    }
    std::array::from_fn(|_| {
        hue = (hue - 90.).rem_euclid(360.);
        let light = lightness / 100.;
        let chroma = saturation * light.min(1. - light) / 100.;
        [0., 8., 4.].map(|offset| {
            let at = (offset + hue / 30.) % 12.;
            (255. * (light - chroma * (at - 3.).min(9. - at).clamp(-1., 1.))).round() as u8
        })
    })
}

fn color_index(red: usize, green: usize, blue: usize) -> usize {
    (red << (2 * SIGBITS)) + (green << SIGBITS) + blue
}

#[derive(Clone, Debug)]
struct VBox {
    r1: i64,
    r2: i64,
    g1: i64,
    g2: i64,
    b1: i64,
    b2: i64,
}

impl VBox {
    fn get(&self, axis: char, end: u8) -> i64 {
        match (axis, end) {
            ('r', 1) => self.r1,
            ('r', _) => self.r2,
            ('g', 1) => self.g1,
            ('g', _) => self.g2,
            ('b', 1) => self.b1,
            _ => self.b2,
        }
    }

    fn set(&mut self, axis: char, end: u8, value: i64) {
        match (axis, end) {
            ('r', 1) => self.r1 = value,
            ('r', _) => self.r2 = value,
            ('g', 1) => self.g1 = value,
            ('g', _) => self.g2 = value,
            ('b', 1) => self.b1 = value,
            _ => self.b2 = value,
        }
    }

    fn volume(&self) -> i64 {
        (self.r2 - self.r1 + 1) * (self.g2 - self.g1 + 1) * (self.b2 - self.b1 + 1)
    }

    fn cells(&self) -> impl Iterator<Item = (i64, i64, i64)> + '_ {
        (self.r1..=self.r2).flat_map(move |red| {
            (self.g1..=self.g2)
                .flat_map(move |green| (self.b1..=self.b2).map(move |blue| (red, green, blue)))
        })
    }

    fn count(&self, histogram: &[u32]) -> u64 {
        self.cells()
            .map(|(red, green, blue)| {
                u64::from(histogram[color_index(red as usize, green as usize, blue as usize)])
            })
            .sum()
    }

    /// `avg`: count-weighted cell centres, truncated, or the box centre when empty.
    fn average(&self, histogram: &[u32]) -> [u8; 3] {
        let mut total = 0f64;
        let (mut red_sum, mut green_sum, mut blue_sum) = (0f64, 0f64, 0f64);
        let step = f64::from(1u32 << RSHIFT);
        for (red, green, blue) in self.cells() {
            let count =
                f64::from(histogram[color_index(red as usize, green as usize, blue as usize)]);
            total += count;
            red_sum += count * (red as f64 + 0.5) * step;
            green_sum += count * (green as f64 + 0.5) * step;
            blue_sum += count * (blue as f64 + 0.5) * step;
        }
        if total > 0. {
            [
                (red_sum / total) as u8,
                (green_sum / total) as u8,
                (blue_sum / total) as u8,
            ]
        } else {
            let middle = |low: i64, high: i64| ((step as i64 * (low + high + 1)) / 2) as u8;
            [
                middle(self.r1, self.r2),
                middle(self.g1, self.g2),
                middle(self.b1, self.b2),
            ]
        }
    }
}

/// The JS `~~`: truncation toward zero.
fn truncate(value: f64) -> i64 {
    value.trunc() as i64
}

/// `medianCutApply`: splits a box at the median of its longest axis, or returns it alone.
fn median_cut(histogram: &[u32], vbox: &VBox) -> Option<(VBox, Option<VBox>)> {
    let total = vbox.count(histogram);
    if total == 0 {
        return None;
    }
    if total == 1 {
        return Some((vbox.clone(), None));
    }
    let red_width = vbox.r2 - vbox.r1 + 1;
    let green_width = vbox.g2 - vbox.g1 + 1;
    let blue_width = vbox.b2 - vbox.b1 + 1;
    let longest = red_width.max(green_width).max(blue_width);
    let axis = if longest == red_width {
        'r'
    } else if longest == green_width {
        'g'
    } else {
        'b'
    };
    // Partial sums along the axis; JS keeps them in a sparse array, so absent means "falsy".
    let (low, high) = (vbox.get(axis, 1), vbox.get(axis, 2));
    let mut partial: std::collections::HashMap<i64, u64> = std::collections::HashMap::new();
    let mut running = 0u64;
    for position in low..=high {
        let mut slice = 0u64;
        let cells: Box<dyn Iterator<Item = (i64, i64, i64)>> = match axis {
            'r' => Box::new(
                (vbox.g1..=vbox.g2)
                    .flat_map(move |g| (vbox.b1..=vbox.b2).map(move |b| (position, g, b))),
            ),
            'g' => Box::new(
                (vbox.r1..=vbox.r2)
                    .flat_map(move |r| (vbox.b1..=vbox.b2).map(move |b| (r, position, b))),
            ),
            _ => Box::new(
                (vbox.r1..=vbox.r2)
                    .flat_map(move |r| (vbox.g1..=vbox.g2).map(move |g| (r, g, position))),
            ),
        };
        for (red, green, blue) in cells {
            slice += u64::from(histogram[color_index(red as usize, green as usize, blue as usize)]);
        }
        running += slice;
        partial.insert(position, running);
    }
    let truthy = |value: Option<&u64>| value.is_some_and(|value| *value != 0);
    for position in low..=high {
        if (partial[&position] as f64) <= running as f64 / 2. {
            continue;
        }
        let mut first = vbox.clone();
        let mut second = vbox.clone();
        let left = position - low;
        let right = high - position;
        let mut cut = if left <= right {
            (high - 1).min(truncate(position as f64 + right as f64 / 2.))
        } else {
            low.max(truncate(position as f64 - 1. - left as f64 / 2.))
        };
        while !truthy(partial.get(&cut)) {
            cut += 1;
        }
        let remainder = |at: i64| partial.get(&at).map(|sum| running - sum);
        let mut count2 = remainder(cut);
        while count2.unwrap_or(0) == 0 && truthy(partial.get(&(cut - 1))) {
            cut -= 1;
            count2 = remainder(cut);
        }
        first.set(axis, 2, cut);
        second.set(axis, 1, cut + 1);
        return Some((first, Some(second)));
    }
    None
}

/// `iter`: keeps splitting the top box until `target` boxes exist.
fn iterate(histogram: &[u32], queue: &mut Vec<VBox>, by_volume: bool, target: f64) {
    let sort = |queue: &mut Vec<VBox>| {
        // Array.prototype.sort is stable; so is sort_by.
        queue.sort_by(|a, b| {
            let key = |vbox: &VBox| {
                let count = vbox.count(histogram);
                if by_volume {
                    count as i128 * vbox.volume() as i128
                } else {
                    count as i128
                }
            };
            key(a).cmp(&key(b))
        });
    };
    let mut boxes = 1usize;
    let mut iterations = 0usize;
    while iterations < MAX_ITERATIONS {
        sort(queue);
        let Some(vbox) = queue.pop() else {
            return;
        };
        if vbox.count(histogram) == 0 {
            queue.push(vbox);
            iterations += 1;
            continue;
        }
        let Some((first, second)) = median_cut(histogram, &vbox) else {
            return;
        };
        queue.push(first);
        if let Some(second) = second {
            queue.push(second);
            boxes += 1;
        }
        if boxes as f64 >= target {
            return;
        }
        let over = iterations > MAX_ITERATIONS;
        iterations += 1;
        if over {
            return;
        }
    }
}

/// `quantize(pixels, count).palette()`: up to `count` colors, largest count times volume first.
pub fn quantize(pixels: &[[u8; 3]], count: usize) -> Option<Vec<[u8; 3]>> {
    if pixels.is_empty() || !(2..=256).contains(&count) {
        return None;
    }
    let mut histogram = vec![0u32; 1 << (3 * SIGBITS)];
    for pixel in pixels {
        let index = color_index(
            usize::from(pixel[0] >> RSHIFT),
            usize::from(pixel[1] >> RSHIFT),
            usize::from(pixel[2] >> RSHIFT),
        );
        histogram[index] += 1;
    }
    // `vboxFromPixels`: the else-if leaves the maximum alone whenever a value lowers the minimum.
    let (mut r1, mut r2, mut g1, mut g2, mut b1, mut b2) =
        (1_000_000i64, 0i64, 1_000_000i64, 0i64, 1_000_000i64, 0i64);
    for pixel in pixels {
        let (red, green, blue) = (
            i64::from(pixel[0] >> RSHIFT),
            i64::from(pixel[1] >> RSHIFT),
            i64::from(pixel[2] >> RSHIFT),
        );
        if red < r1 {
            r1 = red;
        } else if red > r2 {
            r2 = red;
        }
        if green < g1 {
            g1 = green;
        } else if green > g2 {
            g2 = green;
        }
        if blue < b1 {
            b1 = blue;
        } else if blue > b2 {
            b2 = blue;
        }
    }
    let mut queue = vec![VBox {
        r1,
        r2,
        g1,
        g2,
        b1,
        b2,
    }];
    iterate(
        &histogram,
        &mut queue,
        false,
        FRACT_BY_POPULATIONS * count as f64,
    );
    queue.sort_by(|a, b| {
        let key = |vbox: &VBox| vbox.count(&histogram) as i128 * vbox.volume() as i128;
        key(a).cmp(&key(b))
    });
    let remaining = count as f64 - queue.len() as f64;
    iterate(&histogram, &mut queue, true, remaining);
    queue.sort_by(|a, b| {
        let key = |vbox: &VBox| vbox.count(&histogram) as i128 * vbox.volume() as i128;
        key(a).cmp(&key(b))
    });
    // The color map is filled by popping the largest first and reads back in that order.
    Some(
        queue
            .iter()
            .rev()
            .map(|vbox| vbox.average(&histogram))
            .collect(),
    )
}

/// `u(image, 5, 10)`: every tenth pixel that is at least half opaque (alpha 125) and not near white.
pub fn avatar_palette(rgba: &[u8]) -> Vec<[u8; 3]> {
    let (whole, _) = rgba.as_chunks::<4>();
    let pixels: Vec<[u8; 3]> = whole
        .iter()
        .step_by(10)
        .filter(|pixel| pixel[3] >= 125 && !(pixel[0] > 250 && pixel[1] > 250 && pixel[2] > 250))
        .map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect();
    quantize(&pixels, 5).unwrap_or_else(|| vec![[0, 0, 0]])
}

#[cfg(test)]
mod tests {
    #[test]
    fn screening_gradient_companion_matches_the_captured_palette() {
        assert_eq!(super::complementary_colors([65, 58, 46])[0], [82, 29, 75]);
    }
    use super::{avatar_palette, quantize};

    #[test]
    fn a_single_color_comes_back_as_its_cell_centre() {
        let pixels = vec![[88u8, 101, 242]; 200];
        let palette = quantize(&pixels, 5).unwrap();
        // 88>>3=11 -> 92, 101>>3=12 -> 100, 242>>3=30 -> 244: the banner color of the default avatar.
        assert_eq!(palette[0], [92, 100, 244]);
    }

    #[test]
    fn white_and_transparent_pixels_are_left_out() {
        let mut rgba = Vec::new();
        for index in 0..400 {
            // Only every tenth pixel is read: the sampled ones alternate white, transparent and blue.
            let pixel: [u8; 4] = match index % 30 {
                0 => [255, 255, 255, 255],
                10 => [10, 10, 10, 0],
                20 => [88, 101, 242, 255],
                _ => [200, 0, 0, 255],
            };
            rgba.extend_from_slice(&pixel);
        }
        assert_eq!(avatar_palette(&rgba)[0], [92, 100, 244]);
    }
}
