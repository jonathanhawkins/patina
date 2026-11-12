//! `BitMap` — a boolean matrix stored one bit per element (pat-0q2kx.257).
//!
//! Godot ground truth: `upstream/godot/scene/resources/bit_map.cpp` over
//! `doc/classes/BitMap.xml`. This is a faithful port, including the bit layout
//! (`ofs = width * y + x`, byte `ofs / 8`, **LSB-first** within the byte), so a
//! `data` payload written by Godot reads back identically here.
//!
//! Its main consumer is `TextureButton.texture_click_mask`, which uses the mask
//! to reject clicks over transparent pixels.
//!
//! ## Two deliberate deviations, both documented at their site
//!
//! 1. **`BitMapRect` instead of `Rect2i`.** `gdcore::math` has `Vector2i` but no
//!    `Rect2i`, and adding one to `math.rs` is a change to the hottest shared
//!    file in the workspace for a type this bead does not otherwise need. The
//!    local integer rect below is the same half-open rectangle, converts from
//!    `Rect2` (which is what Godot's own docs pass to `opaque_to_polygons`), and
//!    is a mechanical swap if `Rect2i` ever lands.
//! 2. **Out-of-bounds is silent, not a panic.** Godot's `get_bit`/`set_bit` use
//!    `ERR_FAIL_INDEX`, which prints and returns `false` / does nothing. Reads
//!    outside the map therefore return `false` here rather than panicking —
//!    `grow_mask` *relies* on this, because it probes neighbours that can fall
//!    outside the bitmap whenever the caller's rect is larger than the map.

use crate::image::{Image, ImageFormat};
use gdcore::math::{Color, Rect2, Vector2, Vector2i};
use gdvariant::{Dictionary, Variant};

/// Godot's `CMP_EPSILON2` (`core/math/math_defs.h:50-51`) — `CMP_EPSILON`
/// squared. `grow_mask` subtracts it from a squared distance so a neighbour
/// exactly `pixels` away counts as inside the radius.
const CMP_EPSILON2: f32 = 0.00001 * 0.00001;

/// A half-open integer rectangle: covers `x .. x + width` by `y .. y + height`.
///
/// Stands in for Godot's `Rect2i` — see this module's header for why it is local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitMapRect {
    /// Left edge (inclusive).
    pub x: i32,
    /// Top edge (inclusive).
    pub y: i32,
    /// Width; may be zero or negative, in which case the rect covers nothing.
    pub width: i32,
    /// Height; may be zero or negative, in which case the rect covers nothing.
    pub height: i32,
}

impl BitMapRect {
    /// A rect at `(x, y)` of the given size.
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Truncates a float `Rect2` to integer bounds. Godot's docs call
    /// `opaque_to_polygons(Rect2(Vector2(), get_size()))`, relying on the
    /// implicit `Rect2 -> Rect2i` conversion; this is that conversion.
    pub fn from_rect2(r: Rect2) -> Self {
        Self::new(
            r.position.x as i32,
            r.position.y as i32,
            r.size.x as i32,
            r.size.y as i32,
        )
    }

    /// `true` when the rect encloses at least one cell.
    pub const fn has_area(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Half-open containment, matching `Rect2i::has_point`: the top-left edge is
    /// inside, the bottom-right edge is not.
    pub const fn has_point(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    /// The overlap of two rects, or a zero-area rect when they do not touch.
    pub fn intersection(&self, other: &Self) -> Self {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.width).min(other.x + other.width);
        let y1 = (self.y + self.height).min(other.y + other.height);
        if x1 <= x0 || y1 <= y0 {
            return Self::new(x0, y0, 0, 0);
        }
        Self::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// A two-dimensional array of booleans, one bit per element.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitMap {
    width: i32,
    height: i32,
    /// `ceil(width * height / 8)` bytes, bit `n` of the map living in
    /// `bitmask[n / 8]` at position `n % 8` counting from the LSB.
    bitmask: Vec<u8>,
}

impl BitMap {
    /// An empty bitmap — what `BitMap.new()` produces before `create`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `BitMap.create(size)` — a `size` bitmap filled with `false`.
    ///
    /// Godot's `ERR_FAIL_COND` returns *before* resizing, so a degenerate size
    /// leaves the bitmap exactly as it was rather than clearing it. Mirrored.
    pub fn create(&mut self, size: Vector2i) {
        if size.x < 1 || size.y < 1 {
            return;
        }
        // Godot rejects an allocation past INT32_MAX bits.
        if (size.x as i64) * (size.y as i64) > i32::MAX as i64 {
            return;
        }
        self.width = size.x;
        self.height = size.y;
        let bits = size.x as usize * size.y as usize;
        self.bitmask = vec![0u8; bits.div_ceil(8)];
    }

    /// `BitMap.create_from_image_alpha(image, threshold)`.
    ///
    /// A bit is `true` where the image's alpha is **strictly greater** than
    /// `threshold`, and `false` where it is equal or less — note the asymmetry,
    /// which is Godot's (`bit_map.cpp:64`) and matches BitMap.xml's wording.
    /// Godot's default threshold is `0.1`; see [`Self::create_from_image_alpha_default`].
    ///
    /// Godot converts to `FORMAT_LA8` and divides the raw alpha byte by 255;
    /// `Image::get_pixel` already returns alpha normalised to `0.0..=1.0`, and
    /// yields `1.0` for formats without an alpha channel — the same value that
    /// conversion would produce — so the comparison is done directly.
    pub fn create_from_image_alpha(&mut self, image: &Image, threshold: f32) {
        if image.is_empty() {
            return;
        }
        self.create(Vector2i::new(
            image.get_width() as i32,
            image.get_height() as i32,
        ));
        for y in 0..self.height {
            for x in 0..self.width {
                if image.get_pixel(x as u32, y as u32).a > threshold {
                    self.set_bit(x, y, true);
                }
            }
        }
    }

    /// [`Self::create_from_image_alpha`] with Godot's documented default
    /// `threshold` of `0.1`.
    pub fn create_from_image_alpha_default(&mut self, image: &Image) {
        self.create_from_image_alpha(image, 0.1);
    }

    /// `BitMap.get_size()`.
    pub const fn get_size(&self) -> Vector2i {
        Vector2i::new(self.width, self.height)
    }

    /// The rect covering the whole bitmap — the argument Godot's docs suggest
    /// for [`Self::opaque_to_polygons`].
    pub const fn full_rect(&self) -> BitMapRect {
        BitMapRect::new(0, 0, self.width, self.height)
    }

    /// The raw bit payload, for serialization. See [`Self::to_data_dict`].
    pub fn bitmask(&self) -> &[u8] {
        &self.bitmask
    }

    /// The bit index of `(x, y)`, or `None` when out of bounds.
    fn bit_index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        Some(self.width as usize * y as usize + x as usize)
    }

    /// `BitMap.get_bit(x, y)`. Out of bounds reads `false` (Godot's
    /// `ERR_FAIL_INDEX_V(..., false)`) — `grow_mask` depends on this.
    pub fn get_bit(&self, x: i32, y: i32) -> bool {
        match self.bit_index(x, y) {
            Some(ofs) => self
                .bitmask
                .get(ofs / 8)
                .is_some_and(|b| b & (1 << (ofs % 8)) != 0),
            None => false,
        }
    }

    /// `BitMap.get_bitv(position)`.
    pub fn get_bitv(&self, position: Vector2i) -> bool {
        self.get_bit(position.x, position.y)
    }

    /// `BitMap.set_bit(x, y, bit)`. Out of bounds is ignored.
    pub fn set_bit(&mut self, x: i32, y: i32, bit: bool) {
        let Some(ofs) = self.bit_index(x, y) else {
            return;
        };
        let Some(byte) = self.bitmask.get_mut(ofs / 8) else {
            return;
        };
        if bit {
            *byte |= 1 << (ofs % 8);
        } else {
            *byte &= !(1 << (ofs % 8));
        }
    }

    /// `BitMap.set_bitv(position, bit)`.
    pub fn set_bitv(&mut self, position: Vector2i, bit: bool) {
        self.set_bit(position.x, position.y, bit);
    }

    /// `BitMap.set_bit_rect(rect, bit)` — sets every cell in `rect`, clipped to
    /// the bitmap.
    pub fn set_bit_rect(&mut self, rect: BitMapRect, bit: bool) {
        let r = self.full_rect().intersection(&rect);
        for x in r.x..r.x + r.width {
            for y in r.y..r.y + r.height {
                self.set_bit(x, y, bit);
            }
        }
    }

    /// `BitMap.get_true_bit_count()`.
    ///
    /// ⚠️ Counts every bit in the byte array, **including the padding bits** of
    /// a trailing partial byte — which is exactly what Godot does
    /// (`bit_map.cpp:93`, "fast, almost branchless"). `create` zeroes the buffer
    /// and `set_bit` only ever addresses valid offsets, so padding stays 0 and
    /// the count is the true-cell count. It can differ only for a hand-built
    /// `data` payload with dirty padding, and there Godot reports the same
    /// inflated number, so matching it is the parity-correct choice.
    pub fn get_true_bit_count(&self) -> i64 {
        self.bitmask.iter().map(|b| b.count_ones() as i64).sum()
    }

    /// `BitMap.resize(new_size)` — nearest-neighbour resample, up or down.
    pub fn resize(&mut self, new_size: Vector2i) {
        if new_size.x < 0 || new_size.y < 0 || new_size == self.get_size() {
            return;
        }
        let mut resized = BitMap::new();
        resized.create(new_size);
        // Godot guards on the *old* extent being zero, not the new one.
        let lw = if self.width == 0 { 0 } else { new_size.x };
        let lh = if self.height == 0 { 0 } else { new_size.y };
        if new_size.x > 0 && new_size.y > 0 {
            let scale_x = self.width as f32 / new_size.x as f32;
            let scale_y = self.height as f32 / new_size.y as f32;
            for x in 0..lw {
                for y in 0..lh {
                    let bit =
                        self.get_bit((x as f32 * scale_x) as i32, (y as f32 * scale_y) as i32);
                    resized.set_bit(x, y, bit);
                }
            }
        }
        *self = resized;
    }

    /// `BitMap.grow_mask(pixels, rect)` — morphological dilation for positive
    /// `pixels`, erosion for negative. Cells outside `rect` are never written.
    ///
    /// Ported literally from `bit_map.cpp:553`, including two subtleties that a
    /// paraphrase loses:
    ///
    /// - The **skip test reads the live map** (`self.get_bit`) while the
    ///   **neighbour test reads a snapshot** taken before the sweep. Reading the
    ///   live map for neighbours would let this pass's own writes seed further
    ///   growth, dilating by more than `pixels`.
    /// - When eroding, a neighbour outside `rect` counts as *unset* and so
    ///   satisfies the search; when dilating, it is skipped entirely. That
    ///   asymmetry is why the `outside` flag exists rather than a bounds check.
    pub fn grow_mask(&mut self, pixels: i32, rect: BitMapRect) {
        if pixels == 0 {
            return;
        }
        let bit_value = pixels > 0;
        let pixels = pixels.abs();
        let pixels2 = (pixels * pixels) as f32;
        let r = self.full_rect().intersection(&rect);
        let snapshot = self.clone();

        for i in r.y..r.y + r.height {
            for j in r.x..r.x + r.width {
                if bit_value == self.get_bit(j, i) {
                    continue;
                }
                let mut found = false;
                'rows: for y in i - pixels..=i + pixels {
                    for x in j - pixels..=j + pixels {
                        let mut outside = false;
                        if !rect.has_point(x, y) {
                            if bit_value {
                                continue;
                            }
                            outside = true;
                        }
                        let dx = (j - x) as f32;
                        let dy = (i - y) as f32;
                        if dx * dx + dy * dy - CMP_EPSILON2 > pixels2 {
                            continue;
                        }
                        if outside || bit_value == snapshot.get_bit(x, y) {
                            found = true;
                            break 'rows;
                        }
                    }
                }
                if found {
                    self.set_bit(j, i, bit_value);
                }
            }
        }
    }

    /// `BitMap.convert_to_image()` — an `L8` image, white where the bit is set.
    pub fn convert_to_image(&self) -> Image {
        let mut image = Image::create(
            self.width.max(0) as u32,
            self.height.max(0) as u32,
            ImageFormat::L8,
        );
        for x in 0..self.width {
            for y in 0..self.height {
                let v = if self.get_bit(x, y) { 1.0 } else { 0.0 };
                image.set_pixel(x as u32, y as u32, Color::new(v, v, v, 1.0));
            }
        }
        image
    }

    // -- serialization (`_set_data` / `_get_data`) ---------------------------
    //
    // A `.tscn` stores a BitMap sub-resource as
    // `data = { "size": Vector2i(w, h), "data": PackedByteArray(...) }`, so this
    // is the seam a scene-loaded click mask arrives through.

    /// `BitMap._get_data()`.
    pub fn to_data_dict(&self) -> Dictionary {
        let mut d = Dictionary::new();
        d.insert(
            Variant::String("size".into()),
            Variant::Vector2i(self.get_size()),
        );
        d.insert(
            Variant::String("data".into()),
            Variant::PackedByteArray(self.bitmask.clone()),
        );
        d
    }

    /// `BitMap._set_data(dict)` — rebuilds from a `size` + `data` dictionary.
    ///
    /// Returns `None` when either key is missing or of the wrong type, rather
    /// than yielding a half-built map. A short `data` payload is accepted and
    /// zero-padded: Godot assigns the array as-is, and every read then goes
    /// through `get_bit`, which reports `false` for a byte that isn't there.
    pub fn from_data_dict(dict: &Dictionary) -> Option<Self> {
        let size = match dict.get(Variant::String("size".into()))? {
            Variant::Vector2i(v) => *v,
            // A hand-written `.tscn` may spell the size as a float Vector2.
            Variant::Vector2(v) => Vector2i::new(v.x as i32, v.y as i32),
            _ => return None,
        };
        let data = match dict.get(Variant::String("data".into()))? {
            Variant::PackedByteArray(b) => b.clone(),
            _ => return None,
        };
        let mut map = Self::new();
        map.create(size);
        if map.width == 0 || map.height == 0 {
            return None;
        }
        let needed = map.bitmask.len();
        let mut bits = data;
        bits.resize(needed, 0);
        map.bitmask = bits;
        Some(map)
    }

    // -- opaque_to_polygons -------------------------------------------------

    /// `BitMap.opaque_to_polygons(rect, epsilon)` — marching squares over the
    /// set cells, then Ramer–Douglas–Peucker reduction of each outline.
    ///
    /// Godot's method is bound to `clip_opaque_to_polygons`
    /// (`bit_map.cpp:523`). Returns one `Vec<Vector2>` per polygon, in bitmap
    /// coordinates relative to `rect`'s origin. Polygons that reduce below three
    /// vertices are dropped, as Godot drops them.
    pub fn opaque_to_polygons(&self, rect: BitMapRect, epsilon: f32) -> Vec<Vec<Vector2>> {
        let r = self.full_rect().intersection(&rect);
        let mut fill = BitMap::new();
        fill.create(self.get_size());
        let mut polygons: Vec<Vec<Vector2>> = Vec::new();

        for i in r.y..r.y + r.height {
            for j in r.x..r.x + r.width {
                if !fill.get_bit(j, i) && self.get_bit(j, i) {
                    fill_bits(self, &mut fill, j, i, &r);
                    for polygon in self.march_square(&r, j, i) {
                        let reduced = reduce(&polygon, &r, epsilon);
                        if reduced.len() < 3 {
                            // Godot: `print_verbose("Invalid polygon, skipped")`.
                            continue;
                        }
                        polygons.push(reduced);
                    }
                }
            }
        }
        polygons
    }

    /// [`Self::opaque_to_polygons`] with Godot's documented default `epsilon`
    /// of `2.0`.
    pub fn opaque_to_polygons_default(&self, rect: BitMapRect) -> Vec<Vec<Vector2>> {
        self.opaque_to_polygons(rect, 2.0)
    }

    /// Traces the outline containing `(start_x, start_y)` by marching squares.
    ///
    /// Element 0 of the result is the main outline; a self-crossing shape
    /// contributes additional loops, split off at the crossing. Returns empty on
    /// a degenerate start (a fully set or fully clear 2×2 neighbourhood) or if
    /// the walk fails to close within `2 * (w * h + 1)` steps — both are Godot's
    /// `ERR_FAIL_COND_V` bail-outs.
    fn march_square(&self, rect: &BitMapRect, start_x: i32, start_y: i32) -> Vec<Vec<Vector2>> {
        let mut step_x: i32;
        let mut step_y: i32;
        let (mut prev_x, mut prev_y) = (0i32, 0i32);
        let (mut cur_x, mut cur_y) = (start_x, start_y);
        let mut count: i64 = 0;

        // Godot uses an *ordered* HashMap here and walks it from the back when
        // unwinding a crossing, so insertion order is part of the algorithm — an
        // ordered assoc list reproduces it exactly where a HashMap would not.
        //
        // ⚠️ The stored index is `isize`, not `usize`. Godot stores
        // `points_size - 1` as a signed int, and a saddle on the very first step
        // (possible: the start cell is set, so sv >= 8, and sv == 9 needs only
        // the top-left neighbour set) makes that **-1**. As `usize` it would
        // wrap to `usize::MAX` and the `value + 1` slice below would panic on
        // overflow in a debug build.
        let mut cross_map: Vec<((i32, i32), isize)> = Vec::new();
        let mut points: Vec<Vector2> = Vec::new();
        let mut points_size = 0usize;
        let mut ret: Vec<Vec<Vector2>> = vec![Vec::new()];

        loop {
            // The 2x2 neighbourhood, weighted
            //   +---+---+
            //   | 1 | 2 |
            //   +---+---+
            //   | 4 | 8 |   <- 8 is the current cell
            //   +---+---+
            let mut sv = 0;
            if rect.has_point(cur_x - 1, cur_y - 1) && self.get_bit(cur_x - 1, cur_y - 1) {
                sv += 1;
            }
            if rect.has_point(cur_x, cur_y - 1) && self.get_bit(cur_x, cur_y - 1) {
                sv += 2;
            }
            if rect.has_point(cur_x - 1, cur_y) && self.get_bit(cur_x - 1, cur_y) {
                sv += 4;
            }
            if rect.has_point(cur_x, cur_y) && self.get_bit(cur_x, cur_y) {
                sv += 8;
            }
            if sv == 0 || sv == 15 {
                return Vec::new();
            }

            match sv {
                // UP
                1 | 5 | 13 => {
                    step_x = 0;
                    step_y = -1;
                }
                // DOWN
                8 | 10 | 11 => {
                    step_x = 0;
                    step_y = 1;
                }
                // LEFT
                4 | 12 | 14 => {
                    step_x = -1;
                    step_y = 0;
                }
                // RIGHT
                2 | 3 | 7 => {
                    step_x = 1;
                    step_y = 0;
                }
                // Saddle: down if we came from the left, else up.
                9 => {
                    step_x = 0;
                    step_y = if prev_x == 1 { 1 } else { -1 };
                }
                // Saddle: right if we came from below, else left.
                6 => {
                    step_x = if prev_y == -1 { 1 } else { -1 };
                    step_y = 0;
                }
                _ => return Vec::new(),
            }

            // A saddle is where an outline touches itself; the second visit
            // closes a loop, which is split off as its own polygon.
            if sv == 6 || sv == 9 {
                let cur_pos = (cur_x, cur_y);
                if let Some(idx) = cross_map.iter().position(|(p, _)| *p == cur_pos) {
                    let resume = (cross_map[idx].1 + 1).max(0) as usize;
                    ret.push(points[resume.min(points_size)..points_size].to_vec());
                    points_size = resume.min(points_size);
                    // Godot erases every entry after this one, then this one.
                    cross_map.truncate(idx);
                } else {
                    cross_map.push((cur_pos, points_size as isize - 1));
                }
            }

            cur_x += step_x;
            cur_y += step_y;
            let next = Vector2::new((cur_x - rect.x) as f32, (cur_y - rect.y) as f32);
            if step_x == prev_x && step_y == prev_y && points_size > 0 {
                // Same heading as the last step: extend the current segment
                // instead of adding a collinear vertex.
                points[points_size - 1] = next;
            } else if points_size < points.len() {
                points[points_size] = next;
                points_size += 1;
            } else {
                points.push(next);
                points_size += 1;
            }

            count += 1;
            prev_x = step_x;
            prev_y = step_y;
            if count > 2 * (self.width as i64 * self.height as i64 + 1) {
                return Vec::new();
            }
            if cur_x == start_x && cur_y == start_y {
                break;
            }
        }

        points.truncate(points_size);
        ret[0] = points;
        ret
    }
}

/// Flood-fills `map` with every set cell of `src` reachable from `(x, y)`
/// (8-connected) within `rect`.
///
/// Godot uses an explicit stack rather than recursion to survive large bitmaps;
/// this uses one too, for the same reason — a 4096² mask would otherwise blow
/// the stack. The simpler formulation is safe here because the visited set is
/// `map` itself, so a cell is pushed at most once.
fn fill_bits(src: &BitMap, map: &mut BitMap, x: i32, y: i32, rect: &BitMapRect) {
    let mut stack = vec![(x, y)];
    while let Some((px, py)) = stack.pop() {
        for i in px - 1..=px + 1 {
            for j in py - 1..=py + 1 {
                if !rect.has_point(i, j) || map.get_bit(i, j) || !src.get_bit(i, j) {
                    continue;
                }
                map.set_bit(i, j, true);
                stack.push((i, j));
            }
        }
    }
}

/// Distance from `i` to the infinite line through `start` and `end`.
///
/// Ported including its axis special-cases: a vertical line has no finite
/// slope, so it is handled separately rather than dividing by zero.
fn perpendicular_distance(i: Vector2, start: Vector2, end: Vector2) -> f32 {
    if start.x == end.x {
        (i.x - end.x).abs()
    } else if start.y == end.y {
        (i.y - end.y).abs()
    } else {
        let slope = (end.y - start.y) / (end.x - start.x);
        let intercept = start.y - (slope * start.x);
        (slope * i.x - i.y + intercept).abs() / (slope * slope + 1.0).sqrt()
    }
}

/// Ramer–Douglas–Peucker: drops vertices that sit within `optimization` of the
/// chord spanning the run.
fn rdp(v: &[Vector2], optimization: f32) -> Vec<Vector2> {
    if v.len() < 3 {
        return v.to_vec();
    }
    let mut index: isize = -1;
    let mut dist = 0.0f32;
    // The endpoints are always kept, so they are not candidates.
    for i in 1..v.len() - 1 {
        let cdist = perpendicular_distance(v[i], v[0], v[v.len() - 1]);
        if cdist > dist {
            dist = cdist;
            index = i as isize;
        }
    }
    if dist > optimization && index > 0 {
        let index = index as usize;
        let mut left = rdp(&v[..index], optimization);
        let right = rdp(&v[index..], optimization);
        left.extend(right);
        left
    } else {
        vec![v[0], v[v.len() - 1]]
    }
}

/// Reduces one traced outline, clamping `epsilon` to the rect as Godot does.
fn reduce(points: &[Vector2], rect: &BitMapRect, epsilon: f32) -> Vec<Vector2> {
    let size = points.len();
    if size < 3 {
        return Vec::new();
    }
    // Fewer than 9 points is already minimal enough to leave alone.
    if size < 9 {
        return points.to_vec();
    }
    let max_ep = rect.width.min(rect.height) as f32;
    let ep = epsilon.clamp(0.0, max_ep / 2.0);
    let mut result = rdp(points, ep);

    // Godot folds a near-closed tail back into the head so the polygon does not
    // end with two coincident vertices.
    if result.len() >= 2 {
        let last = result[result.len() - 1];
        let first = result[0];
        let d = ((last.x - first.x).powi(2) + (last.y - first.y).powi(2)).sqrt();
        if last.y > first.y && d < ep * 0.5 {
            result[0].y = last.y;
            result.pop();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x4 image whose left half is opaque and right half fully transparent.
    fn half_transparent() -> Image {
        let mut img = Image::create(4, 4, ImageFormat::Rgba8);
        for y in 0..4 {
            for x in 0..4 {
                let a = if x < 2 { 1.0 } else { 0.0 };
                img.set_pixel(x, y, Color::new(1.0, 1.0, 1.0, a));
            }
        }
        img
    }

    #[test]
    fn create_zeroes_and_sizes() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(10, 3));
        assert_eq!(b.get_size(), Vector2i::new(10, 3));
        assert_eq!(b.get_true_bit_count(), 0);
        // 30 bits needs 4 bytes, not 3.
        assert_eq!(b.bitmask().len(), 4);
    }

    #[test]
    fn create_rejects_degenerate_size_without_clearing() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(4, 4));
        b.set_bit(0, 0, true);
        b.create(Vector2i::new(0, 5));
        assert_eq!(
            b.get_size(),
            Vector2i::new(4, 4),
            "a rejected create must leave the map untouched, as ERR_FAIL_COND does"
        );
        assert!(b.get_bit(0, 0), "and must not clear existing bits");
    }

    #[test]
    fn bit_layout_is_lsb_first_row_major() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(8, 2));
        b.set_bit(0, 0, true); // bit 0 -> byte 0, mask 0x01
        b.set_bit(1, 0, true); // bit 1 -> byte 0, mask 0x02
        b.set_bit(0, 1, true); // bit 8 -> byte 1, mask 0x01
        assert_eq!(b.bitmask()[0], 0b0000_0011);
        assert_eq!(b.bitmask()[1], 0b0000_0001);
        assert_eq!(b.get_true_bit_count(), 3);
    }

    #[test]
    fn out_of_bounds_reads_false_and_writes_nothing() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(2, 2));
        assert!(!b.get_bit(-1, 0));
        assert!(!b.get_bit(0, 99));
        b.set_bit(-1, -1, true);
        b.set_bit(50, 50, true);
        assert_eq!(
            b.get_true_bit_count(),
            0,
            "out-of-bounds writes must be dropped, not wrap into a valid cell"
        );
    }

    #[test]
    fn create_from_image_alpha_threshold_is_exclusive() {
        let mut b = BitMap::new();
        b.create_from_image_alpha(&half_transparent(), 0.1);
        assert_eq!(b.get_size(), Vector2i::new(4, 4));
        assert_eq!(
            b.get_true_bit_count(),
            8,
            "the opaque half is 2 of 4 columns"
        );
        assert!(b.get_bit(0, 0) && b.get_bit(1, 3));
        assert!(!b.get_bit(2, 0) && !b.get_bit(3, 3));

        // Alpha exactly equal to the threshold is FALSE (`> threshold`).
        //
        // 🔑 The alpha MUST be a value `Rgba8` can hold exactly, and `0.5` is
        // not one (pat-21oab). This assertion was red at HEAD for exactly that
        // reason: `set_pixel` quantizes with `(v * 255.0 + 0.5) as u8`
        // (image.rs), and 0.5 × 255 = 127.5 is not an integer, so 0.5 stored as
        // **128** and `get_pixel` read back 128/255 = 0.50196… — which really is
        // `> 0.5`, so the bit really was set and the test could never pass. The
        // implementation was right the whole time; the fixture asked an 8-bit
        // channel for a value it cannot represent.
        //
        // `128.0 / 255.0` round-trips bit-for-bit: it stores 128 and reads back
        // the identical `f32`, which is what makes "exactly equal" testable at
        // all here.
        let exact = 128.0 / 255.0;
        let mut img = Image::create(1, 1, ImageFormat::Rgba8);
        img.set_pixel(0, 0, Color::new(1.0, 1.0, 1.0, exact));
        let mut eq = BitMap::new();
        eq.create_from_image_alpha(&img, exact);
        assert!(
            !eq.get_bit(0, 0),
            "alpha == threshold must be false: Godot tests `alpha > threshold`"
        );
        let mut below = BitMap::new();
        below.create_from_image_alpha(&img, exact - 0.01);
        assert!(
            below.get_bit(0, 0),
            "alpha just above the threshold is true"
        );
    }

    #[test]
    fn alphaless_format_is_fully_opaque() {
        // An RGB8 image has no alpha; Godot's convert-to-LA8 fills it with 255,
        // so every bit must come out set rather than clear.
        let img = Image::create(3, 3, ImageFormat::Rgb8);
        let mut b = BitMap::new();
        b.create_from_image_alpha(&img, 0.1);
        assert_eq!(b.get_true_bit_count(), 9);
    }

    #[test]
    fn set_bit_rect_clips_to_the_map() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(4, 4));
        b.set_bit_rect(BitMapRect::new(-2, -2, 4, 4), true);
        assert_eq!(
            b.get_true_bit_count(),
            4,
            "only the 2x2 overlap with the map is written"
        );
        assert!(b.get_bit(0, 0) && b.get_bit(1, 1) && !b.get_bit(2, 2));
    }

    #[test]
    fn grow_mask_dilates_and_erodes() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(7, 7));
        b.set_bit(3, 3, true);
        b.grow_mask(1, b.full_rect());
        // A radius-1 dilation of one cell is the 3x3 block around it: the
        // diagonals are at distance sqrt(2) > 1 and must NOT be included.
        assert!(b.get_bit(2, 3) && b.get_bit(4, 3) && b.get_bit(3, 2) && b.get_bit(3, 4));
        assert!(
            !b.get_bit(2, 2),
            "sqrt(2) exceeds a 1px radius, so corners stay clear"
        );
        assert_eq!(b.get_true_bit_count(), 5);

        b.grow_mask(-1, b.full_rect());
        assert_eq!(
            b.get_true_bit_count(),
            1,
            "eroding by the same radius returns the single cell"
        );
        assert!(b.get_bit(3, 3));
    }

    #[test]
    fn grow_mask_leaves_cells_outside_the_rect_alone() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(7, 7));
        b.set_bit(3, 3, true);
        // Restrict growth to a rect that excludes the cell above.
        b.grow_mask(1, BitMapRect::new(3, 3, 4, 4));
        assert!(
            !b.get_bit(3, 2),
            "a cell outside the rect must never be written"
        );
        assert!(b.get_bit(4, 3), "a cell inside the rect still grows");
    }

    #[test]
    fn grow_mask_zero_is_a_noop() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(4, 4));
        b.set_bit(1, 1, true);
        b.grow_mask(0, b.full_rect());
        assert_eq!(b.get_true_bit_count(), 1);
    }

    #[test]
    fn data_dict_roundtrips() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(5, 3));
        b.set_bit(4, 2, true);
        b.set_bit(0, 0, true);
        let restored = BitMap::from_data_dict(&b.to_data_dict()).expect("roundtrip");
        assert_eq!(restored, b);
        assert!(restored.get_bit(4, 2) && restored.get_bit(0, 0));
        assert_eq!(restored.get_true_bit_count(), 2);
    }

    #[test]
    fn data_dict_rejects_incomplete_payloads() {
        let mut d = Dictionary::new();
        d.insert(
            Variant::String("size".into()),
            Variant::Vector2i(Vector2i::new(2, 2)),
        );
        assert!(
            BitMap::from_data_dict(&d).is_none(),
            "a dict with no `data` key must not produce a half-built map"
        );

        let mut only_data = Dictionary::new();
        only_data.insert(
            Variant::String("data".into()),
            Variant::PackedByteArray(vec![0xFF]),
        );
        assert!(
            BitMap::from_data_dict(&only_data).is_none(),
            "no `size` key"
        );

        // A short payload is zero-extended rather than rejected.
        let mut short = Dictionary::new();
        short.insert(
            Variant::String("size".into()),
            Variant::Vector2i(Vector2i::new(8, 4)),
        );
        short.insert(
            Variant::String("data".into()),
            Variant::PackedByteArray(vec![0b0000_0001]),
        );
        let m = BitMap::from_data_dict(&short).expect("short payload is padded");
        assert_eq!(m.bitmask().len(), 4);
        assert!(m.get_bit(0, 0));
        assert_eq!(m.get_true_bit_count(), 1);
    }

    #[test]
    fn resize_rescales_the_mask() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(4, 4));
        b.set_bit_rect(BitMapRect::new(0, 0, 2, 4), true); // left half set
        b.resize(Vector2i::new(8, 4));
        assert_eq!(b.get_size(), Vector2i::new(8, 4));
        assert!(
            b.get_bit(0, 0) && b.get_bit(3, 0),
            "the left half stays set after upscaling"
        );
        assert!(
            !b.get_bit(4, 0) && !b.get_bit(7, 0),
            "the right half stays clear"
        );
    }

    #[test]
    fn convert_to_image_is_white_where_set() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(2, 1));
        b.set_bit(0, 0, true);
        let img = b.convert_to_image();
        assert_eq!((img.get_width(), img.get_height()), (2, 1));
        assert!(img.get_pixel(0, 0).r > 0.9, "a set bit is white");
        assert!(img.get_pixel(1, 0).r < 0.1, "a clear bit is black");
    }

    #[test]
    fn opaque_to_polygons_traces_a_square() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(8, 8));
        b.set_bit_rect(BitMapRect::new(2, 2, 4, 4), true);
        let polys = b.opaque_to_polygons(b.full_rect(), 2.0);
        assert_eq!(polys.len(), 1, "one blob traces one polygon, got {polys:?}");
        assert!(
            polys[0].len() >= 3,
            "a polygon needs at least 3 vertices, got {:?}",
            polys[0]
        );
        // Every vertex must lie on the blob's boundary box.
        for v in &polys[0] {
            assert!(
                (2.0..=6.0).contains(&v.x) && (2.0..=6.0).contains(&v.y),
                "vertex {v:?} escaped the 4x4 blob at (2,2)"
            );
        }
    }

    #[test]
    fn opaque_to_polygons_finds_each_disjoint_blob() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(16, 8));
        b.set_bit_rect(BitMapRect::new(1, 1, 3, 3), true);
        b.set_bit_rect(BitMapRect::new(10, 2, 4, 4), true);
        let polys = b.opaque_to_polygons(b.full_rect(), 2.0);
        assert_eq!(polys.len(), 2, "two disjoint blobs, got {}", polys.len());
    }

    #[test]
    fn opaque_to_polygons_on_an_empty_map_is_empty() {
        let mut b = BitMap::new();
        b.create(Vector2i::new(8, 8));
        assert!(b.opaque_to_polygons(b.full_rect(), 2.0).is_empty());
    }
}
