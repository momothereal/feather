use num_traits::ToPrimitive;
use simdnoise::NoiseBuilder;

/// Convenient wrapper over `simdnoise::GradientSettings`
/// which supports SIMD-accelerated amplitude and linear
/// interpolation.
///
/// The `simdnoise::GradientSettings::generate_scaled` function
/// cannot be used for noise amplification because it scales
/// the noise differently depending on the min and max value
/// in a chunk, creating very obvious seams between chunks.
/// For more information,  jackmott/rust-simd-noise#9.
///
/// This algorithm uses a different technique by multiplying
/// noise by a constant value provided in the `new` function.
/// This ensures that amplification remains constant across chunks.
///
/// # Notes
/// * The value initially retrieved from the noise function is
/// not within the range -1.0 to 1.0; it is undefined. Experimentation
/// is needed to obtain a suitable amplitude value for any given set of settings.
/// * The X and Z offsets are multiplied by the horizontal and vertical
/// sizes, respectively, to obtain the offset in absolute coordinates.
/// (This means there is no need to multiply the chunk coordinate by 16.)
pub struct Wrapped3DPerlinNoise {
    /// The seed used for noise generation.
    seed: u64,
    /// The frequency.
    frequency: f32,
    /// The amplitude.
    amplitude: f32,
    /// The size of the chunk to generate along X and Z axes.
    size_horizontal: u32,
    /// The size of the chunk to generate along the Y axis.
    size_vertical: u32,
    /// The offset along the X axis to generate.
    offset_x: i32,
    /// The offset along the Z axis to generate.
    offset_z: i32,
    /// The scale along the X and Z axes. Must be a divisor of size_horizontal.
    scale_horizontal: u32,
    /// The scale along the Y axis. Must be a divisor of size_vertical.
    scale_vertical: u32,
}

impl Wrapped3DPerlinNoise {
    /// Initializes with default settings and the given seed.
    ///
    /// Default settings are intended to match the size
    /// of chunks. Horizontal and vertical size and scale
    /// are initialized to sane defaults.
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            frequency: 0.02,
            amplitude: 400.0,
            size_horizontal: 16,
            size_vertical: 256,
            offset_x: 0,
            offset_z: 0,
            scale_horizontal: 4,
            scale_vertical: 8,
        }
    }

    /// Sets the frequency.
    pub fn with_frequency(mut self, frequency: f32) -> Self {
        self.frequency = frequency;
        self
    }

    /// Sets the amplitude.
    pub fn with_amplitude(mut self, amplitude: f32) -> Self {
        self.amplitude = amplitude;
        self
    }

    /// Sets the size of the chunk to be generated.
    pub fn with_size(mut self, xz: u32, y: u32) -> Self {
        self.size_horizontal = xz;
        self.size_vertical = y;
        self
    }

    /// Sets the X and Z offsets.
    ///
    /// # Notes
    /// * The X and Z offsets are multiplied by the horizontal and vertical
    /// sizes, respectively, to obtain the offset in absolute coordinates.
    /// (This means there is no need to multiply the chunk coordinate by 16.)
    pub fn with_offset(mut self, x: i32, z: i32) -> Self {
        self.offset_x = x;
        self.offset_z = z;
        self
    }

    /// Sets the scale of the noise. Linear interpolation
    /// is used between values based on this scale.
    pub fn with_scale(mut self, horizontal: u32, vertical: u32) -> Self {
        self.scale_horizontal = horizontal;
        self.size_vertical = vertical;
        self
    }

    /// Generates a linear-interpolated block of noise.
    /// The returned vector will have length `size_horizontal^2 * size_vertical`,
    /// indexable by `((y << 12) | z << 4) | x`.
    pub fn generate(&self) -> Vec<f32> {
        // If AVX2 is available, use it. Otherwise,
        // default to a scalar impl.
        // TODO: support SSE41, other SIMD instruction sets

        if is_x86_feature_detected!("avx2") {
            self.generate_avx2()
        } else {
            self.generate_fallback()
        }
    }

    fn generate_avx2(&self) -> Vec<f32> {
        // TODO: implement this. (Premature optimization is bad!)
        self.generate_fallback()
    }

    fn generate_fallback(&self) -> Vec<f32> {
        // Loop through values ofsetted by the scale.
        // Then, loop through all coordinates inside
        // that subchunk and apply linear interpolation.

        // This is based on Glowstone's OverworldGenerator.generateRawTerrain
        // with a few modifications and superior variable names.

        // Number of subchunks in a chunk along each axis.
        let subchunk_horizontal = self.size_horizontal / self.scale_horizontal;
        let subchunk_vertical = self.size_vertical / self.scale_vertical;

        // Density noise, with one value every `scale` blocks along each axis.
        // Indexing into this vector is done using `self.uninterpolated_index(x, y, z)`.
        let (mut densities, _, _) = NoiseBuilder::gradient_3d_offset(
            (self.size_horizontal as i32 * self.offset_x / self.scale_horizontal as i32) as f32,
            (subchunk_horizontal + 1) as usize,
            0.0,
            (subchunk_vertical + 1) as usize,
            (self.size_horizontal as i32 * self.offset_z / self.scale_horizontal as i32) as f32,
            (subchunk_horizontal + 1) as usize,
        )
        .with_freq(self.frequency)
        .with_seed(self.seed as i32)
        .generate();

        // Apply amplitude to density.
        densities.iter_mut().for_each(|x| *x *= self.amplitude);

        // Buffer to emit final noise into.
        // TODO: consider using Vec::set_len to avoid zeroing it out
        let mut buf =
            vec![0.0; (self.size_horizontal * self.size_horizontal * self.size_vertical) as usize];

        let scale_vertical = self.scale_vertical as f32;
        let scale_horizontal = self.scale_horizontal as f32;

        // Coordinates of the subchunk. The subchunk
        // is the chunk within the chunk in which we
        // only find the noise value for the corners
        // and then apply interpolation in between.

        // Here, we loop through the subchunks and interpolate
        // noise for each block within it.
        for subx in 0..subchunk_horizontal {
            for suby in 0..subchunk_vertical {
                for subz in 0..subchunk_horizontal {
                    // Two grids of noise values:
                    // one for the four bottom corners
                    // of the subchunk, and one for the
                    // offsets along the Y axis to apply
                    // to those base corners each block increment.

                    // These are mutated so that they are at the
                    // current Y position.
                    let mut base1 = densities[self.uninterpolated_index(subx, suby, subz)];
                    let mut base2 = densities[self.uninterpolated_index(subx + 1, suby, subz)];
                    let mut base3 = densities[self.uninterpolated_index(subx, suby, subz + 1)];
                    let mut base4 = densities[self.uninterpolated_index(subx + 1, suby, subz + 1)];

                    // Offsets for each block along the Y axis from each corner above.
                    let offset1 = (densities[self.uninterpolated_index(subx, suby + 1, subz)]
                        - base1)
                        / scale_vertical;
                    let offset2 = (densities[self.uninterpolated_index(subx + 1, suby + 1, subz)]
                        - base2)
                        / scale_vertical;
                    let offset3 = (densities[self.uninterpolated_index(subx, suby + 1, subz + 1)]
                        - base3)
                        / scale_vertical;
                    let offset4 = (densities
                        [self.uninterpolated_index(subx + 1, suby + 1, subz + 1)]
                        - base4)
                        / scale_vertical;

                    // Iterate through the blocks in this subchunk
                    // and apply interpolation before setting the
                    // noise value in the final buffer.
                    for blocky in 0..self.scale_vertical {
                        let mut z_base = base1;
                        let mut z_corner = base3;
                        for blockx in 0..self.scale_horizontal {
                            let mut density = z_base;
                            for blockz in 0..self.scale_horizontal {
                                // Set interpolated value in buffer.
                                buf[index(
                                    blockx + (self.scale_horizontal * subx),
                                    blocky + (self.scale_vertical * suby),
                                    blockz + (self.scale_horizontal * subz),
                                )] = density;

                                // Apply Z interpolation.
                                density += (z_corner - z_base) / scale_horizontal;
                            }
                            // Interpolation along X.
                            z_base += (base2 - base1) / scale_horizontal;
                            // Along Z again.
                            z_corner += (base4 - base3) / scale_horizontal;
                        }

                        // Interpolation along Y.
                        base1 += offset1;
                        base2 += offset2;
                        base3 += offset3;
                        base4 += offset4;
                    }
                }
            }
        }

        buf
    }

    fn uninterpolated_index<N: ToPrimitive>(&self, x: N, y: N, z: N) -> usize {
        let length = (self.size_horizontal / self.scale_horizontal + 1) as usize;

        let x = x.to_usize().unwrap();
        let y = y.to_usize().unwrap();
        let z = z.to_usize().unwrap();

        (y * (length * length) + (z * length) + x)
    }
}

pub fn index<N: ToPrimitive>(x: N, y: N, z: N) -> usize {
    let x = x.to_usize().unwrap();
    let y = y.to_usize().unwrap();
    let z = z.to_usize().unwrap();

    ((y << 8) | z << 4) | x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_test() {
        let noise = Wrapped3DPerlinNoise::new(0)
            .with_amplitude(400.0)
            .with_offset(10, 16);

        let chunk = noise.generate();

        assert_eq!(chunk.len(), 16 * 256 * 16);
    }
}
