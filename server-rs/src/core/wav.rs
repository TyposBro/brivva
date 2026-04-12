//! PCM to WAV conversion utility.

use crate::core::config::{SAMPLE_RATE, BITS_PER_SAMPLE, CHANNELS};

pub fn pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    pcm_to_wav_at(pcm, SAMPLE_RATE)
}

pub fn pcm_to_wav_at(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let byte_rate = sample_rate * (BITS_PER_SAMPLE as u32 / 8) * CHANNELS as u32;
    let block_align = CHANNELS * (BITS_PER_SAMPLE / 8);
    let data_size = pcm.len() as u32;

    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&CHANNELS.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_produce_valid_wav_header_for_empty_pcm() {
        let wav = pcm_to_wav(&[]);

        assert_eq!(wav.len(), 44);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1); // PCM format
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), CHANNELS);
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), SAMPLE_RATE);
    }

    #[test]
    fn should_embed_pcm_data_after_header() {
        let pcm = [0x01, 0x02, 0x03, 0x04];
        let wav = pcm_to_wav(&pcm);

        assert_eq!(&wav[44..48], &pcm);
    }

    #[test]
    fn should_set_data_size_to_pcm_length() {
        let pcm = vec![0u8; 100];
        let wav = pcm_to_wav(&pcm);

        let data_size = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
        assert_eq!(data_size, 100);
    }

    #[test]
    fn should_set_riff_size_to_data_plus_36() {
        let pcm = vec![0u8; 200];
        let wav = pcm_to_wav(&pcm);

        let riff_size = u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]);
        assert_eq!(riff_size, 200 + 36);
    }
}
