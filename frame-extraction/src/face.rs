use image::{RgbImage, imageops};

pub struct FaceCropper {
    face_size: u32,
}

impl FaceCropper {
    pub fn new(face_size: u32) -> Self {
        Self { face_size }
    }

    pub fn crop_center(&self, frame: &RgbImage) -> RgbImage {
        let (w, h) = frame.dimensions();
        let side = w.min(h);

        let x = (w - side) / 2;
        let y = (h - side) / 2;

        let cropped = imageops::crop_imm(frame, x, y, side, side).to_image();

        if side == self.face_size {
            cropped
        } else {
            imageops::resize(&cropped, self.face_size, self.face_size, imageops::FilterType::Triangle)
        }
    }
}
