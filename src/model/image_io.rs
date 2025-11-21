#[cfg(feature = "heif")]
pub unsafe fn load_heif(path: &std::path::PathBuf) -> color_eyre::eyre::Result<opencv::core::Mat> {
    use color_eyre::eyre::eyre;
    use opencv::core::{self, MatTrait, MatTraitConst};

    libheif_sys::heif_init(std::ptr::null_mut());
    let ctx = libheif_sys::heif_context_alloc();
    let bytes = std::fs::read(&path).map_err(|e| eyre!("Failed to read file bytes: {e}"))?;
    let size = bytes.len();
    let ptr = bytes.as_ptr() as *mut std::ffi::c_void;
    let err = libheif_sys::heif_context_read_from_memory(ctx, ptr, size, std::ptr::null());
    if err.code != libheif_sys::heif_error_code_heif_error_Ok {
        libheif_sys::heif_context_free(ctx);
        return Err(eyre!(
            "Failed to read HEIF image: {}",
            std::ffi::CStr::from_ptr(err.message).to_string_lossy()
        ));
    }

    let mut handle = std::ptr::null_mut();
    let err = libheif_sys::heif_context_get_primary_image_handle(ctx, &mut handle);

    if err.code != libheif_sys::heif_error_code_heif_error_Ok {
        libheif_sys::heif_context_free(ctx);
        return Err(eyre!(
            "Failed to get HEIF image handle: {}",
            std::ffi::CStr::from_ptr(err.message).to_string_lossy()
        ));
    }

    let width = libheif_sys::heif_image_handle_get_width(handle);
    let height = libheif_sys::heif_image_handle_get_height(handle);
    let has_alpha = libheif_sys::heif_image_handle_has_alpha_channel(handle) != 0;

    let num_channels = if has_alpha { 4 } else { 3 };
    let cvtype = match num_channels {
        3 => core::CV_8UC3,
        4 => core::CV_8UC4,
        _ => {
            libheif_sys::heif_context_free(ctx);
            return Err(eyre!("Unsupported number of channels in HEIF image: {}", num_channels));
        }
    };

    let mut mat = core::Mat::new_rows_cols(height as i32, width as i32, cvtype)?;

    let mut image = std::ptr::null_mut();
    let options = libheif_sys::heif_decoding_options_alloc();
    let err = libheif_sys::heif_decode_image(
        handle,
        &mut image,
        libheif_sys::heif_colorspace_heif_colorspace_RGB,
        libheif_sys::heif_chroma_heif_chroma_interleaved_RGB,
        options,
    );
    libheif_sys::heif_decoding_options_free(options);

    if err.code != libheif_sys::heif_error_code_heif_error_Ok {
        libheif_sys::heif_image_handle_release(handle);
        libheif_sys::heif_context_free(ctx);
        return Err(eyre!(
            "Failed to decode HEIF image: {}",
            std::ffi::CStr::from_ptr(err.message).to_string_lossy()
        ));
    }

    let mut stride: i32 = 0;
    let src_ptr = libheif_sys::heif_image_get_plane_readonly(
        image,
        libheif_sys::heif_channel_heif_channel_interleaved,
        &mut stride,
    );
    if src_ptr.is_null() {
        libheif_sys::heif_image_release(image);
        libheif_sys::heif_image_handle_release(handle);
        libheif_sys::heif_context_free(ctx);
        return Err(eyre!("Failed to get HEIF image plane"));
    }

    let dst_ptr = mat.data_mut();
    let dst_step = mat.step1(0)? as usize;

    for y in 0..height {
        let src_row = src_ptr.add((y * stride) as usize);
        let dst_row = dst_ptr.add(y as usize * dst_step);
        let bytes_per_row = (width as usize) * num_channels;

        std::ptr::copy_nonoverlapping(src_row, dst_row, bytes_per_row);
    }

    libheif_sys::heif_image_release(image);
    libheif_sys::heif_image_handle_release(handle);
    libheif_sys::heif_context_free(ctx);

    Ok(mat)
}
