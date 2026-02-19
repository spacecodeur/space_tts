use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use crossbeam_channel::Sender;
use rubato::Resampler;

pub struct CaptureConfig {
    pub sample_rate: u32,
    pub channels: u16,
}

pub fn start_capture(
    device: &cpal::Device,
    sender: Sender<Vec<i16>>,
) -> Result<(cpal::Stream, CaptureConfig)> {
    let config = device
        .default_input_config()
        .context("Failed to get default input config")?;

    let sample_rate = config.sample_rate();
    let channels = config.channels();

    let stream_config: cpal::StreamConfig = config.into();

    let err_fn = |err: cpal::StreamError| {
        eprintln!("Audio stream error: {err}");
    };

    let stream = device
        .build_input_stream(
            &stream_config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let _ = sender.try_send(data.to_vec());
            },
            err_fn,
            None,
        )
        .context("Failed to build input stream")?;

    stream.play().context("Failed to start audio stream")?;

    Ok((
        stream,
        CaptureConfig {
            sample_rate,
            channels,
        },
    ))
}

pub fn create_resampler(
    source_rate: u32,
    target_rate: u32,
    channels: u16,
) -> Result<Box<dyn FnMut(&[i16]) -> Vec<i16>>> {
    if source_rate == target_rate && channels == 1 {
        return Ok(Box::new(|samples: &[i16]| samples.to_vec()));
    }

    let ch = channels as usize;
    let ratio = target_rate as f64 / source_rate as f64;

    use rubato::{
        Async, FixedAsync, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };

    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Quadratic,
        oversampling_factor: 256,
        window: WindowFunction::Blackman2,
    };

    let chunk_size = 1024;
    let mut resampler =
        Async::<f64>::new_sinc(ratio, 1.1, &params, chunk_size, 1, FixedAsync::Input)
            .map_err(|e| anyhow::anyhow!("Failed to create resampler: {e}"))?;

    Ok(Box::new(move |samples: &[i16]| {
        // Convert to mono f64 normalized [-1.0, 1.0]
        let mono: Vec<f64> = if ch == 1 {
            samples.iter().map(|&s| s as f64 / 32768.0).collect()
        } else {
            samples
                .chunks(ch)
                .map(|frame| {
                    let sum: f64 = frame.iter().map(|&s| s as f64).sum();
                    (sum / ch as f64) / 32768.0
                })
                .collect()
        };

        // Process in chunk_size frames, collect all output
        let mut output_all: Vec<i16> = Vec::new();
        let mut offset = 0;

        while offset < mono.len() {
            let end = (offset + chunk_size).min(mono.len());
            let chunk = &mono[offset..end];

            // Pad to chunk_size if needed (last partial chunk)
            let padded: Vec<f64>;
            let input_slice: &[f64] = if chunk.len() < chunk_size {
                padded = {
                    let mut v = chunk.to_vec();
                    v.resize(chunk_size, 0.0);
                    v
                };
                &padded
            } else {
                chunk
            };

            let input_data: Vec<Vec<f64>> = vec![input_slice.to_vec()];
            use audioadapter_buffers::direct::SequentialSliceOfVecs;
            let adapter = SequentialSliceOfVecs::new(&input_data, 1, chunk_size).unwrap();

            match resampler.process(&adapter, 0, None) {
                Ok(output) => {
                    let samples: Vec<f64> = output.take_data();
                    let actual_out = if chunk.len() < chunk_size {
                        let expected = (chunk.len() as f64 * ratio).ceil() as usize;
                        &samples[..expected.min(samples.len())]
                    } else {
                        &samples[..]
                    };
                    for &s in actual_out {
                        let clamped = s.clamp(-1.0, 1.0);
                        output_all.push((clamped * 32767.0) as i16);
                    }
                }
                Err(e) => {
                    eprintln!("Resample error: {e}");
                }
            }

            offset = end;
        }

        output_all
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_noop_mono() {
        let mut resample = create_resampler(16000, 16000, 1).unwrap();
        let input: Vec<i16> = (0..1600).collect();
        let output = resample(&input);
        assert_eq!(output, input);
    }

    #[test]
    fn resampler_48k_to_16k() {
        let mut resample = create_resampler(48000, 16000, 1).unwrap();
        // 100ms at 48kHz = 4800 samples
        let input: Vec<i16> = vec![0; 4800];
        let output = resample(&input);
        // Expected ~1600 samples (100ms at 16kHz), allow some margin
        let expected = 1600;
        let margin = 200;
        assert!(
            (output.len() as i32 - expected as i32).unsigned_abs() < margin,
            "Expected ~{expected} samples, got {}",
            output.len()
        );
    }
}
