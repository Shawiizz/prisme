//! Streams in whatever sample format the device uses, seen as `f32` by Prisme.

use crate::AudioError;
use cpal::traits::DeviceTrait;
use cpal::{FromSample, Sample, SampleFormat, SizedSample, I24};

/// Largest period we prepare conversion buffers for (frames).
pub(crate) const MAX_PERIOD: usize = 16_384;

/// Builds an output stream; `render` fills interleaved `f32` frames.
pub(crate) fn output_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    format: SampleFormat,
    render: impl FnMut(&mut [f32]) + Send + 'static,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, AudioError> {
    match format {
        SampleFormat::F32 => output_as::<f32>(device, config, render, on_error),
        SampleFormat::I32 => output_as::<i32>(device, config, render, on_error),
        SampleFormat::I24 => output_as::<I24>(device, config, render, on_error),
        SampleFormat::I16 => output_as::<i16>(device, config, render, on_error),
        other => Err(AudioError::UnsupportedFormat(other)),
    }
}

/// Builds an input stream; `push` receives interleaved `f32` frames.
pub(crate) fn input_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    format: SampleFormat,
    push: impl FnMut(&[f32]) + Send + 'static,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, AudioError> {
    match format {
        SampleFormat::F32 => input_as::<f32>(device, config, push, on_error),
        SampleFormat::I32 => input_as::<i32>(device, config, push, on_error),
        SampleFormat::I24 => input_as::<I24>(device, config, push, on_error),
        SampleFormat::I16 => input_as::<i16>(device, config, push, on_error),
        other => Err(AudioError::UnsupportedFormat(other)),
    }
}

fn output_as<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut render: impl FnMut(&mut [f32]) + Send + 'static,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, AudioError> {
    let mut scratch = vec![0.0f32; MAX_PERIOD * config.channels as usize];
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            let n = data.len().min(scratch.len());
            render(&mut scratch[..n]);
            for (out, &s) in data.iter_mut().zip(&scratch[..n]) {
                *out = T::from_sample(s);
            }
        },
        on_error,
        None,
    )?;
    Ok(stream)
}

fn input_as<T: SizedSample>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut push: impl FnMut(&[f32]) + Send + 'static,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, AudioError>
where
    f32: FromSample<T>,
{
    let mut scratch = vec![0.0f32; MAX_PERIOD * config.channels as usize];
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            let n = data.len().min(scratch.len());
            for (out, &s) in scratch[..n].iter_mut().zip(data) {
                *out = f32::from_sample(s);
            }
            push(&scratch[..n]);
        },
        on_error,
        None,
    )?;
    Ok(stream)
}
