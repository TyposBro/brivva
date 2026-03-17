use image::{imageops, Rgb, RgbImage};

pub struct CropRegion {
    pub x: u32,
    pub y: u32,
    pub size: u32,
}

pub struct FaceCropper {
    face_size: u32,
}

impl FaceCropper {
    pub fn new(face_size: u32) -> Self {
        Self { face_size }
    }

    /// Returns the crop region (in original frame coordinates)
    pub fn crop_region(&self, frame: &RgbImage) -> CropRegion {
        let (w, h) = frame.dimensions();
        let side = w.min(h);
        CropRegion {
            x: (w - side) / 2,
            y: (h - side) / 2,
            size: side,
        }
    }

    pub fn crop_center(&self, frame: &RgbImage) -> RgbImage {
        let r = self.crop_region(frame);
        let cropped = imageops::crop_imm(frame, r.x, r.y, r.size, r.size).to_image();

        if r.size == self.face_size {
            cropped
        } else {
            imageops::resize(
                &cropped,
                self.face_size,
                self.face_size,
                imageops::FilterType::Triangle,
            )
        }
    }
}

/// Draw a rectangle border on an RgbImage (no extra deps needed)
pub fn draw_rect(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, color: Rgb<u8>, thickness: u32) {
    let (iw, ih) = img.dimensions();
    for t in 0..thickness {
        // Top and bottom edges
        for px in x.saturating_sub(t)..=(x + w + t).min(iw - 1) {
            if y >= t {
                img.put_pixel(px.min(iw - 1), y - t, color);
            }
            if y + h + t < ih {
                img.put_pixel(px.min(iw - 1), y + h + t, color);
            }
        }
        // Left and right edges
        for py in y.saturating_sub(t)..=(y + h + t).min(ih - 1) {
            if x >= t {
                img.put_pixel(x - t, py.min(ih - 1), color);
            }
            if x + w + t < iw {
                img.put_pixel(x + w + t, py.min(ih - 1), color);
            }
        }
    }
}
