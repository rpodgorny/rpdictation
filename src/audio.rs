use anyhow::{Context, Result};
use flacenc::component::BitRepr;
use flacenc::error::Verify;

pub fn samples_to_wav(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer =
            hound::WavWriter::new(&mut cursor, spec).context("Failed to create WAV writer")?;
        for &sample in samples {
            writer.write_sample(sample)?;
        }
        writer.finalize().context("Failed to finalize WAV")?;
    }
    Ok(cursor.into_inner())
}

pub fn wav_to_flac(wav_data: &[u8], sample_rate: u32) -> Result<Vec<u8>> {
    // Parse WAV file to get PCM samples
    let mut cursor = std::io::Cursor::new(wav_data);
    let reader = hound::WavReader::new(&mut cursor).context("Failed to parse WAV data")?;

    let samples: Vec<i32> = reader
        .into_samples::<i16>()
        .map(|s| s.map(|x| x as i32))
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to read WAV samples")?;

    // Encode to FLAC
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|(_, e)| anyhow::anyhow!("Failed to verify FLAC encoder config: {:?}", e))?;

    let source = flacenc::source::MemSource::from_samples(
        &samples,
        1,  // mono
        16, // bits per sample
        sample_rate as usize,
    );

    let flac_stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| anyhow::anyhow!("Failed to encode FLAC: {:?}", e))?;

    // Write to bytes
    let mut sink = flacenc::bitsink::ByteSink::new();
    flac_stream
        .write(&mut sink)
        .map_err(|e| anyhow::anyhow!("Failed to write FLAC data: {:?}", e))?;

    Ok(sink.as_slice().to_vec())
}
