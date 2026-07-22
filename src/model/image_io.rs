use crate::model::PixelType;
use color_eyre::eyre::{eyre, Result};
use exr::prelude::{read, MetaData, ReadChannels, ReadLayers, ReadSpecificChannel, Text, Vec2};
use image::{DynamicImage, ImageFormat, ImageReader};
use std::{
    fs::File,
    io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom},
    path::Path,
    sync::Once,
};
use tiff::{
    decoder::{BufferLayoutPreference, Decoder as TiffDecoder, DecodingResult, Limits as TiffLimits},
    ColorType as TiffColorType,
};

const EXR_MAGIC: [u8; 4] = [0x76, 0x2f, 0x31, 0x01];
const JP2_MAGIC: [u8; 8] = [0x00, 0x00, 0x00, 0x0c, 0x6a, 0x50, 0x20, 0x20];
const J2C_MAGIC: [u8; 4] = [0xff, 0x4f, 0xff, 0x51];
const TIFF_LE_MAGIC: [u8; 4] = [b'I', b'I', 42, 0];
const TIFF_BE_MAGIC: [u8; 4] = [b'M', b'M', 0, 42];
const BIG_TIFF_LE_MAGIC: [u8; 4] = [b'I', b'I', 43, 0];
const BIG_TIFF_BE_MAGIC: [u8; 4] = [b'M', b'M', 0, 43];

static DECODER_HOOKS: Once = Once::new();

fn ensure_decoder_hooks() {
    DECODER_HOOKS.call_once(|| {
        hayro_jpeg2000::integration::register_decoding_hook();
    });
}

pub struct DecodedImage {
    pub width: i32,
    pub height: i32,
    pub channels: i32,
    pub pixel_type: PixelType,
    pub pixels: Vec<f32>,
}

impl DecodedImage {
    fn new(width: u32, height: u32, channels: i32, pixel_type: PixelType, pixels: Vec<f32>) -> Result<Self> {
        let width = i32::try_from(width).map_err(|_| eyre!("Image width exceeds i32: {width}"))?;
        let height = i32::try_from(height).map_err(|_| eyre!("Image height exceeds i32: {height}"))?;
        let expected = width as usize * height as usize * channels as usize;
        if pixels.len() != expected {
            return Err(eyre!("Unexpected decoded image length: {} != {expected}", pixels.len()));
        }
        Ok(Self {
            width,
            height,
            channels,
            pixel_type,
            pixels,
        })
    }
}

pub fn decode_path(path: &Path) -> Result<DecodedImage> {
    ensure_decoder_hooks();
    let mut reader = BufReader::new(File::open(path)?);
    let magic = reader.fill_buf()?;
    if magic.starts_with(&EXR_MAGIC) {
        return decode_exr_reader(reader);
    }
    if is_jpeg2000(magic) {
        return decode_jpeg2000_reader(reader);
    }
    if is_tiff(magic) {
        return decode_tiff_reader(reader);
    }
    let mut image_reader = ImageReader::new(reader).with_guessed_format()?;
    if image_reader.format().is_none() {
        if let Ok(format) = ImageFormat::from_path(path) {
            image_reader.set_format(format);
        }
    }
    let image = image_reader.decode()?;
    decoded_dynamic_image(image)
}

pub fn decode_bytes(bytes: &[u8]) -> Result<DecodedImage> {
    ensure_decoder_hooks();
    if bytes.starts_with(&EXR_MAGIC) {
        return decode_exr_reader(Cursor::new(bytes));
    }
    if is_jpeg2000(bytes) {
        return decode_jpeg2000_reader(Cursor::new(bytes));
    }
    if is_tiff(bytes) {
        return decode_tiff_reader(Cursor::new(bytes));
    }
    let image_reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let image = if image_reader.format().is_some() {
        image_reader.decode()?
    } else {
        // TGA deliberately has no fixed magic signature. Try its strict header
        // parser only after every signature-based format has been ruled out.
        ImageReader::with_format(Cursor::new(bytes), ImageFormat::Tga).decode()?
    };
    decoded_dynamic_image(image)
}

fn is_jpeg2000(bytes: &[u8]) -> bool {
    bytes.starts_with(&JP2_MAGIC) || bytes.starts_with(&J2C_MAGIC)
}

fn is_tiff(bytes: &[u8]) -> bool {
    [TIFF_LE_MAGIC, TIFF_BE_MAGIC, BIG_TIFF_LE_MAGIC, BIG_TIFF_BE_MAGIC]
        .iter()
        .any(|magic| bytes.starts_with(magic))
}

#[derive(Clone, Copy)]
enum ExrLayout {
    Rgb,
    Rgba,
    Mono(&'static str),
}

#[derive(Clone, Copy)]
struct ExrSelection {
    layout: ExrLayout,
    width: usize,
    height: usize,
    data_offset_x: i32,
    data_offset_y: i32,
}

impl ExrSelection {
    fn channels(self) -> usize {
        match self.layout {
            ExrLayout::Rgb => 3,
            ExrLayout::Rgba => 4,
            ExrLayout::Mono(_) => 1,
        }
    }

    fn value_count(self) -> Result<usize> {
        self.width
            .checked_mul(self.height)
            .and_then(|pixels| pixels.checked_mul(self.channels()))
            .ok_or_else(|| eyre!("EXR image size overflow"))
    }
}

struct ExrPixels {
    values: Vec<f32>,
    selection: ExrSelection,
}

impl ExrPixels {
    fn new(selection: ExrSelection) -> Self {
        Self {
            values: vec![0.0; selection.value_count().expect("validated EXR image size")],
            selection,
        }
    }

    fn set<const N: usize>(&mut self, position: Vec2<usize>, values: [f32; N]) {
        debug_assert_eq!(N, self.selection.channels());
        let x = position.x() as i64 + self.selection.data_offset_x as i64;
        let y = position.y() as i64 + self.selection.data_offset_y as i64;
        if x < 0 || y < 0 || x >= self.selection.width as i64 || y >= self.selection.height as i64 {
            return;
        }
        let start = (y as usize * self.selection.width + x as usize) * N;
        self.values[start..start + N].copy_from_slice(&values);
    }
}

fn decode_exr_reader<R: BufRead + Seek>(reader: R) -> Result<DecodedImage> {
    let exr_reader = exr::block::read(reader, false)?;
    let selection = select_exr_layout(exr_reader.meta_data())?;
    selection.value_count()?;

    let pixels = match selection.layout {
        ExrLayout::Rgb => {
            let image = read()
                .no_deep_data()
                .largest_resolution_level()
                .rgb_channels(
                    move |_resolution, _channels| ExrPixels::new(selection),
                    |pixels, position, (r, g, b): (f32, f32, f32)| pixels.set(position, [r, g, b]),
                )
                .first_valid_layer()
                .all_attributes()
                .from_chunks(exr_reader)?;
            image.layer_data.channel_data.pixels.values
        }
        ExrLayout::Rgba => {
            let image = read()
                .no_deep_data()
                .largest_resolution_level()
                .rgba_channels(
                    move |_resolution, _channels| ExrPixels::new(selection),
                    |pixels, position, (r, g, b, a): (f32, f32, f32, f32)| {
                        pixels.set(position, [r, g, b, a]);
                    },
                )
                .first_valid_layer()
                .all_attributes()
                .from_chunks(exr_reader)?;
            image.layer_data.channel_data.pixels.values
        }
        ExrLayout::Mono(channel_name) => {
            let image = read()
                .no_deep_data()
                .largest_resolution_level()
                .specific_channels()
                .required::<f32>(channel_name)
                .collect_pixels(
                    move |_resolution, _channels| ExrPixels::new(selection),
                    |pixels, position, (value,): (f32,)| pixels.set(position, [value]),
                )
                .first_valid_layer()
                .all_attributes()
                .from_chunks(exr_reader)?;
            image.layer_data.channel_data.pixels.values
        }
    };

    DecodedImage::new(
        u32::try_from(selection.width).map_err(|_| eyre!("EXR width exceeds u32"))?,
        u32::try_from(selection.height).map_err(|_| eyre!("EXR height exceeds u32"))?,
        selection.channels() as i32,
        PixelType::F32,
        pixels,
    )
}

fn select_exr_layout(meta: &MetaData) -> Result<ExrSelection> {
    let has_channel = |header: &exr::meta::header::Header, name: &'static str| {
        header.channels.find_index_of_channel(&Text::from(name)).is_some()
    };

    // Preserve image-rs's existing preference for the first usable RGB layer.
    if let Some(header) = meta
        .headers
        .iter()
        .find(|header| !header.deep && ["R", "G", "B"].into_iter().all(|name| has_channel(header, name)))
    {
        let layout = if has_channel(header, "A") {
            ExrLayout::Rgba
        } else {
            ExrLayout::Rgb
        };
        return exr_selection(header, layout);
    }

    // A float mono image has no representation in image-rs DynamicImage. If
    // there is no RGB layer, retain the first conventional scalar channel
    // (including the common depth channel Z) as one component.
    for header in meta.headers.iter().filter(|header| !header.deep) {
        for channel_name in ["R", "L", "Y", "Z"] {
            if has_channel(header, channel_name) {
                return exr_selection(header, ExrLayout::Mono(channel_name));
            }
        }
    }

    Err(eyre!("EXR has no non-deep RGB/RGBA layer or single R, L, Y, or Z channel"))
}

fn exr_selection(header: &exr::meta::header::Header, layout: ExrLayout) -> Result<ExrSelection> {
    let display_window = header.shared_attributes.display_window;
    let data_offset = header.own_attributes.layer_position - display_window.position;
    let selection = ExrSelection {
        layout,
        width: display_window.size.width(),
        height: display_window.size.height(),
        data_offset_x: data_offset.x(),
        data_offset_y: data_offset.y(),
    };
    if selection.width == 0 || selection.height == 0 {
        return Err(eyre!("EXR has invalid zero-sized display window"));
    }
    selection.value_count()?;
    Ok(selection)
}

fn decode_jpeg2000_reader<R: Read + Seek>(mut reader: R) -> Result<DecodedImage> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    let image = hayro_jpeg2000::Image::new(&bytes, &hayro_jpeg2000::DecodeSettings::default())?;

    // Hayro's image-rs hook always calls `store_u8_into`, even for a 16-bit
    // codestream. Read simple display color spaces from the decoded component
    // planes instead so EdolView never performs the lossy U16 -> U8 -> F32
    // round trip. CMYK and ICC images retain the hook's color-management path.
    if !matches!(
        image.color_space(),
        hayro_jpeg2000::ColorSpace::Gray | hayro_jpeg2000::ColorSpace::RGB
    ) {
        let dynamic = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?.decode()?;
        return decoded_dynamic_image(dynamic);
    }

    let width = image.width();
    let height = image.height();
    let channels = image.color_space().num_channels() as usize + usize::from(image.has_alpha());
    let pixel_count = width as usize * height as usize;
    let value_count = pixel_count
        .checked_mul(channels)
        .ok_or_else(|| eyre!("JPEG 2000 image size overflow"))?;

    let mut context = hayro_jpeg2000::DecoderContext::default();
    let decoded = image.decode(&mut context)?;
    let components = decoded.components();
    if components.len() != channels || components.iter().any(|component| component.samples().len() != pixel_count) {
        return Err(eyre!(
            "Unexpected JPEG 2000 component layout: {} components for {channels} channels",
            components.len()
        ));
    }

    let max_depth = components.iter().map(|component| component.bit_depth()).max().unwrap_or(8);
    let pixel_type = match max_depth {
        0..=8 => PixelType::U8,
        9..=16 => PixelType::U16,
        _ => PixelType::F32,
    };
    let mut pixels = Vec::with_capacity(value_count);
    for pixel in 0..pixel_count {
        for component in components {
            let bit_depth = component.bit_depth();
            let max_value = if bit_depth >= 32 {
                u32::MAX as f32
            } else {
                ((1_u32 << bit_depth) - 1) as f32
            };
            pixels.push(component.samples()[pixel] / max_value);
        }
    }

    DecodedImage::new(width, height, channels as i32, pixel_type, pixels)
}

#[derive(Clone, Copy)]
enum TiffColorLayout {
    Direct { channels: usize },
    Palette,
    Cmyk { alpha: bool },
    YCbCr,
    Lab,
}

impl TiffColorLayout {
    fn input_channels(self) -> usize {
        match self {
            Self::Direct { channels } => channels,
            Self::Palette => 1,
            Self::Cmyk { alpha: false } => 4,
            Self::Cmyk { alpha: true } => 5,
            Self::YCbCr | Self::Lab => 3,
        }
    }

    fn output_channels(self) -> usize {
        match self {
            Self::Direct { channels } => channels,
            Self::Palette | Self::Cmyk { alpha: false } | Self::YCbCr | Self::Lab => 3,
            Self::Cmyk { alpha: true } => 4,
        }
    }
}

fn decode_tiff_reader<R: Read + Seek>(mut reader: R) -> Result<DecodedImage> {
    // tiff 0.11 rejects palette photometric images while constructing the
    // decoder, before callers can retrieve ColorMap. Inspect that small piece
    // of metadata first and only copy/patch the compressed stream for this
    // otherwise unsupported layout.
    let prepatch = inspect_tiff_prepatch(&mut reader)?;
    reader.seek(SeekFrom::Start(0))?;
    if let Some(prepatch) = prepatch {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        let replacement = match prepatch {
            TiffPrepatch::Palette(_) => 1, // BlackIsZero
            TiffPrepatch::Lab { .. } => 2, // RGB
        };
        patch_tiff_photometric(&mut bytes, replacement)?;
        let mut decoder = TiffDecoder::new(Cursor::new(bytes))?;
        let (width, height) = decoder.dimensions()?;
        let (color_type, color_layout, palette) = match prepatch {
            TiffPrepatch::Palette(palette) => (
                TiffColorType::Palette(palette.bit_depth),
                TiffColorLayout::Palette,
                Some(palette.colors),
            ),
            TiffPrepatch::Lab { bit_depth } => (TiffColorType::Lab(bit_depth), TiffColorLayout::Lab, None),
        };
        return decode_tiff_image(decoder, width, height, color_type, color_layout, palette);
    }

    let mut decoder = TiffDecoder::new(reader)?;
    let (width, height) = decoder.dimensions()?;
    let color_type = decoder.colortype()?;
    let color_layout = tiff_color_layout(color_type)?;

    decode_tiff_image(decoder, width, height, color_type, color_layout, None)
}

fn tiff_color_layout(color_type: TiffColorType) -> Result<TiffColorLayout> {
    let layout = match color_type {
        TiffColorType::Gray(_) => TiffColorLayout::Direct { channels: 1 },
        TiffColorType::GrayA(_) => TiffColorLayout::Direct { channels: 2 },
        TiffColorType::RGB(_) => TiffColorLayout::Direct { channels: 3 },
        TiffColorType::RGBA(_) => TiffColorLayout::Direct { channels: 4 },
        TiffColorType::Palette(_) => TiffColorLayout::Palette,
        TiffColorType::CMYK(_) => TiffColorLayout::Cmyk { alpha: false },
        TiffColorType::CMYKA(_) => TiffColorLayout::Cmyk { alpha: true },
        TiffColorType::YCbCr(_) => TiffColorLayout::YCbCr,
        TiffColorType::Lab(_) => TiffColorLayout::Lab,
        TiffColorType::Multiband { num_samples, .. } if num_samples <= 4 => TiffColorLayout::Direct {
            channels: num_samples as usize,
        },
        TiffColorType::Multiband { num_samples, .. } => {
            return Err(eyre!("TIFF has {num_samples} bands; EdolView supports at most four"));
        }
        _ => return Err(eyre!("Unsupported TIFF color layout: {color_type:?}")),
    };
    Ok(layout)
}

#[derive(Clone, Copy)]
enum TiffByteOrder {
    Little,
    Big,
}

impl TiffByteOrder {
    fn u16(self, bytes: &[u8]) -> u16 {
        let bytes = bytes.try_into().expect("two TIFF bytes");
        match self {
            Self::Little => u16::from_le_bytes(bytes),
            Self::Big => u16::from_be_bytes(bytes),
        }
    }

    fn u32(self, bytes: &[u8]) -> u32 {
        let bytes = bytes.try_into().expect("four TIFF bytes");
        match self {
            Self::Little => u32::from_le_bytes(bytes),
            Self::Big => u32::from_be_bytes(bytes),
        }
    }

    fn u64(self, bytes: &[u8]) -> u64 {
        let bytes = bytes.try_into().expect("eight TIFF bytes");
        match self {
            Self::Little => u64::from_le_bytes(bytes),
            Self::Big => u64::from_be_bytes(bytes),
        }
    }
}

struct TiffIfdEntry {
    field_type: u16,
    count: u64,
    value_offset: u64,
    inline: [u8; 8],
    inline_capacity: usize,
}

struct TiffPalette {
    bit_depth: u8,
    colors: Vec<u16>,
}

enum TiffPrepatch {
    Palette(TiffPalette),
    Lab { bit_depth: u8 },
}

fn inspect_tiff_prepatch<R: Read + Seek>(reader: &mut R) -> Result<Option<TiffPrepatch>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut header = [0_u8; 16];
    reader.read_exact(&mut header[..8])?;
    let order = match &header[..2] {
        b"II" => TiffByteOrder::Little,
        b"MM" => TiffByteOrder::Big,
        _ => return Err(eyre!("Invalid TIFF byte order")),
    };
    let big_tiff = match order.u16(&header[2..4]) {
        42 => false,
        43 => true,
        magic => return Err(eyre!("Invalid TIFF magic: {magic}")),
    };
    if big_tiff {
        reader.read_exact(&mut header[8..16])?;
        if order.u16(&header[4..6]) != 8 || order.u16(&header[6..8]) != 0 {
            return Err(eyre!("Unsupported BigTIFF offset layout"));
        }
    }

    let ifd_offset = if big_tiff {
        order.u64(&header[8..16])
    } else {
        order.u32(&header[4..8]) as u64
    };
    reader.seek(SeekFrom::Start(ifd_offset))?;
    let entry_count = if big_tiff {
        let mut count = [0_u8; 8];
        reader.read_exact(&mut count)?;
        order.u64(&count)
    } else {
        let mut count = [0_u8; 2];
        reader.read_exact(&mut count)?;
        order.u16(&count) as u64
    };
    if entry_count > 65_535 {
        return Err(eyre!("TIFF IFD has an unreasonable number of entries: {entry_count}"));
    }

    let mut photometric = None;
    let mut bits = None;
    let mut color_map = None;
    for _ in 0..entry_count {
        let mut raw = [0_u8; 20];
        let entry_len = if big_tiff { 20 } else { 12 };
        reader.read_exact(&mut raw[..entry_len])?;
        let tag = order.u16(&raw[..2]);
        if !matches!(tag, 258 | 262 | 320) {
            continue;
        }
        let field_type = order.u16(&raw[2..4]);
        let (count, value_start, inline_capacity) = if big_tiff {
            (order.u64(&raw[4..12]), 12, 8)
        } else {
            (order.u32(&raw[4..8]) as u64, 8, 4)
        };
        let value_offset = if big_tiff {
            order.u64(&raw[value_start..value_start + 8])
        } else {
            order.u32(&raw[value_start..value_start + 4]) as u64
        };
        let mut inline = [0_u8; 8];
        inline[..inline_capacity].copy_from_slice(&raw[value_start..value_start + inline_capacity]);
        let entry = TiffIfdEntry {
            field_type,
            count,
            value_offset,
            inline,
            inline_capacity,
        };
        match tag {
            258 => bits = Some(entry),
            262 => photometric = Some(entry),
            320 => color_map = Some(entry),
            _ => unreachable!(),
        }
    }

    let Some(photometric) = photometric else {
        return Ok(None);
    };
    let photometric = read_tiff_short_values(reader, order, &photometric, 1)?;
    let photometric = photometric.first().copied();
    if !matches!(photometric, Some(3) | Some(8)) {
        return Ok(None);
    }
    let bits = bits.ok_or_else(|| eyre!("TIFF has no BitsPerSample tag"))?;
    let bit_depth = *read_tiff_short_values(reader, order, &bits, 4)?
        .first()
        .ok_or_else(|| eyre!("TIFF has an empty BitsPerSample tag"))?;
    let bit_depth = u8::try_from(bit_depth).map_err(|_| eyre!("TIFF bit depth is too large"))?;
    if photometric == Some(8) {
        return Ok(Some(TiffPrepatch::Lab { bit_depth }));
    }
    if bit_depth == 0 || bit_depth > 16 {
        return Err(eyre!("Unsupported palette TIFF bit depth: {bit_depth}"));
    }
    let color_map = color_map.ok_or_else(|| eyre!("Palette TIFF has no ColorMap tag"))?;
    let expected_colors = 3_usize
        .checked_mul(1_usize << bit_depth)
        .ok_or_else(|| eyre!("Palette TIFF ColorMap size overflow"))?;
    let colors = read_tiff_short_values(reader, order, &color_map, expected_colors)?;
    if colors.len() < expected_colors {
        return Err(eyre!(
            "Palette TIFF ColorMap is truncated: {} < {expected_colors}",
            colors.len()
        ));
    }
    Ok(Some(TiffPrepatch::Palette(TiffPalette { bit_depth, colors })))
}

fn read_tiff_short_values<R: Read + Seek>(
    reader: &mut R,
    order: TiffByteOrder,
    entry: &TiffIfdEntry,
    maximum: usize,
) -> Result<Vec<u16>> {
    if entry.field_type != 3 {
        return Err(eyre!("Expected TIFF SHORT field, found type {}", entry.field_type));
    }
    let count = usize::try_from(entry.count).map_err(|_| eyre!("TIFF SHORT count overflow"))?;
    if count > maximum {
        return Err(eyre!("TIFF SHORT field is too large: {count} > {maximum}"));
    }
    let byte_count = count.checked_mul(2).ok_or_else(|| eyre!("TIFF SHORT size overflow"))?;
    let mut bytes = vec![0_u8; byte_count];
    if byte_count <= entry.inline_capacity {
        bytes.copy_from_slice(&entry.inline[..byte_count]);
    } else {
        reader.seek(SeekFrom::Start(entry.value_offset))?;
        reader.read_exact(&mut bytes)?;
    }
    Ok(bytes.chunks_exact(2).map(|bytes| order.u16(bytes)).collect())
}

fn decode_tiff_image<R: Read + Seek>(
    mut decoder: TiffDecoder<R>,
    width: u32,
    height: u32,
    color_type: TiffColorType,
    color_layout: TiffColorLayout,
    palette: Option<Vec<u16>>,
) -> Result<DecodedImage> {
    let bit_depth = color_type.bit_depth();

    let pixel_count = width as usize * height as usize;
    let packed_bytes = pixel_count
        .checked_mul(color_layout.input_channels())
        .and_then(|samples| samples.checked_mul(bit_depth as usize))
        .map(|bits| bits.div_ceil(8))
        .ok_or_else(|| eyre!("TIFF image size overflow"))?;
    let mut limits = TiffLimits::default();
    limits.decoding_buffer_size = limits.decoding_buffer_size.max(packed_bytes);
    limits.intermediate_buffer_size = limits.intermediate_buffer_size.max(packed_bytes.min(512 * 1024 * 1024));
    decoder = decoder.with_limits(limits);

    let mut result = DecodingResult::U8(Vec::new());
    let buffer_layout = decoder.read_image_to_buffer(&mut result)?;
    let (raw, pixel_type, sample_bytes) = normalize_tiff_result(result, bit_depth)?;
    let samples = reshape_tiff_samples(
        raw,
        &buffer_layout,
        sample_bytes,
        width as usize,
        height as usize,
        color_layout.input_channels(),
        bit_depth,
    )?;
    let (pixels, pixel_type) = convert_tiff_color(samples, color_layout, palette.as_deref(), bit_depth, pixel_type)?;

    DecodedImage::new(width, height, color_layout.output_channels() as i32, pixel_type, pixels)
}

fn patch_tiff_photometric(bytes: &mut [u8], replacement: u16) -> Result<()> {
    if bytes.len() < 8 {
        return Err(eyre!("Palette TIFF header is truncated"));
    }
    let little = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(eyre!("Invalid TIFF byte order")),
    };
    let read_u16 = |slice: &[u8]| {
        if little {
            u16::from_le_bytes(slice.try_into().expect("two TIFF bytes"))
        } else {
            u16::from_be_bytes(slice.try_into().expect("two TIFF bytes"))
        }
    };
    let read_u32 = |slice: &[u8]| {
        if little {
            u32::from_le_bytes(slice.try_into().expect("four TIFF bytes"))
        } else {
            u32::from_be_bytes(slice.try_into().expect("four TIFF bytes"))
        }
    };
    let read_u64 = |slice: &[u8]| {
        if little {
            u64::from_le_bytes(slice.try_into().expect("eight TIFF bytes"))
        } else {
            u64::from_be_bytes(slice.try_into().expect("eight TIFF bytes"))
        }
    };

    let big_tiff = read_u16(&bytes[2..4]) == 43;
    let (entry_count, entries_start, entry_size) = if big_tiff {
        if bytes.len() < 16 {
            return Err(eyre!("BigTIFF header is truncated"));
        }
        let offset = usize::try_from(read_u64(&bytes[8..16])).map_err(|_| eyre!("BigTIFF IFD offset overflow"))?;
        let count_end = offset.checked_add(8).ok_or_else(|| eyre!("BigTIFF IFD overflow"))?;
        let count = usize::try_from(read_u64(
            bytes.get(offset..count_end).ok_or_else(|| eyre!("BigTIFF IFD is truncated"))?,
        ))
        .map_err(|_| eyre!("BigTIFF entry count overflow"))?;
        (count, count_end, 20)
    } else {
        let offset = read_u32(&bytes[4..8]) as usize;
        let count_end = offset.checked_add(2).ok_or_else(|| eyre!("TIFF IFD overflow"))?;
        let count = read_u16(bytes.get(offset..count_end).ok_or_else(|| eyre!("TIFF IFD is truncated"))?) as usize;
        (count, count_end, 12)
    };

    for index in 0..entry_count {
        let start = entries_start
            .checked_add(index.checked_mul(entry_size).ok_or_else(|| eyre!("TIFF entry overflow"))?)
            .ok_or_else(|| eyre!("TIFF entry overflow"))?;
        let end = start.checked_add(entry_size).ok_or_else(|| eyre!("TIFF entry overflow"))?;
        let entry = bytes.get_mut(start..end).ok_or_else(|| eyre!("TIFF entry is truncated"))?;
        if read_u16(&entry[..2]) == 262 {
            let value_offset = if big_tiff { 12 } else { 8 };
            if little {
                entry[value_offset..value_offset + 2].copy_from_slice(&replacement.to_le_bytes());
            } else {
                entry[value_offset..value_offset + 2].copy_from_slice(&replacement.to_be_bytes());
            }
            return Ok(());
        }
    }
    Err(eyre!("TIFF has no PhotometricInterpretation tag"))
}

fn normalize_tiff_result(result: DecodingResult, bit_depth: u8) -> Result<(Vec<f32>, PixelType, usize)> {
    let unsigned_max = if bit_depth >= 64 {
        u64::MAX as f64
    } else {
        ((1_u64 << bit_depth) - 1) as f64
    };
    let signed_max = if bit_depth >= 64 {
        i64::MAX as f64
    } else {
        ((1_i64 << bit_depth.saturating_sub(1)) - 1).max(1) as f64
    };
    let converted = match result {
        DecodingResult::U8(values) => (
            values
                .into_iter()
                .map(|value| value as f32 / if bit_depth < 8 { 255.0 } else { unsigned_max as f32 })
                .collect(),
            PixelType::U8,
            1,
        ),
        DecodingResult::U16(values) => (
            values.into_iter().map(|value| value as f32 / unsigned_max as f32).collect(),
            PixelType::U16,
            2,
        ),
        DecodingResult::U32(values) => (
            values.into_iter().map(|value| (value as f64 / unsigned_max) as f32).collect(),
            PixelType::F32,
            4,
        ),
        DecodingResult::U64(values) => (
            values.into_iter().map(|value| (value as f64 / unsigned_max) as f32).collect(),
            PixelType::F32,
            8,
        ),
        DecodingResult::I8(values) => (
            values.into_iter().map(|value| value as f32 / signed_max as f32).collect(),
            PixelType::I8,
            1,
        ),
        DecodingResult::I16(values) => (
            values.into_iter().map(|value| value as f32 / signed_max as f32).collect(),
            PixelType::I16,
            2,
        ),
        DecodingResult::I32(values) => (
            values.into_iter().map(|value| (value as f64 / signed_max) as f32).collect(),
            PixelType::I32,
            4,
        ),
        DecodingResult::I64(values) => (
            values.into_iter().map(|value| (value as f64 / signed_max) as f32).collect(),
            PixelType::F32,
            8,
        ),
        DecodingResult::F16(values) => (values.into_iter().map(|value| value.to_f32()).collect(), PixelType::F16, 2),
        DecodingResult::F32(values) => (values, PixelType::F32, 4),
        DecodingResult::F64(values) => (values.into_iter().map(|value| value as f32).collect(), PixelType::F64, 8),
    };
    Ok(converted)
}

fn reshape_tiff_samples(
    values: Vec<f32>,
    layout: &BufferLayoutPreference,
    sample_bytes: usize,
    width: usize,
    height: usize,
    channels: usize,
    bit_depth: u8,
) -> Result<Vec<f32>> {
    if bit_depth < 8 {
        // Sub-byte samples occupy packed bytes, so the normalized f32 values
        // above correspond one-to-one with source bytes. Recover the byte and
        // expand MSB-first samples while respecting TIFF row padding.
        let bytes: Vec<u8> = values.iter().map(|value| (value * 255.0).round() as u8).collect();
        let row_stride = layout
            .row_stride
            .map_or_else(|| (width * channels * bit_depth as usize).div_ceil(8), |stride| stride.get());
        let plane_stride = layout.plane_stride.map_or(row_stride * height, |stride| stride.get());
        let max_value = ((1_u16 << bit_depth) - 1) as f32;
        let mask = (1_u8 << bit_depth) - 1;
        let mut output = Vec::with_capacity(width * height * channels);
        for y in 0..height {
            for x in 0..width {
                for channel in 0..channels {
                    let (base, sample) = if layout.planes > 1 {
                        (channel * plane_stride + y * row_stride, x)
                    } else {
                        (y * row_stride, x * channels + channel)
                    };
                    let bit = sample * bit_depth as usize;
                    let byte = *bytes.get(base + bit / 8).ok_or_else(|| eyre!("Packed TIFF row is truncated"))?;
                    let shift = 8 - bit_depth as usize - bit % 8;
                    output.push(((byte >> shift) & mask) as f32 / max_value);
                }
            }
        }
        return Ok(output);
    }

    let row_stride = layout.row_stride.map_or(width * channels, |stride| stride.get() / sample_bytes);
    let plane_stride = layout
        .plane_stride
        .map_or(row_stride * height, |stride| stride.get() / sample_bytes);
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| eyre!("TIFF sample count overflow"))?;
    if layout.planes <= 1 && row_stride == width * channels && values.len() == expected {
        return Ok(values);
    }
    let mut output = Vec::with_capacity(width * height * channels);
    for y in 0..height {
        for x in 0..width {
            for channel in 0..channels {
                let index = if layout.planes > 1 {
                    channel * plane_stride + y * row_stride + x
                } else {
                    y * row_stride + x * channels + channel
                };
                output.push(*values.get(index).ok_or_else(|| eyre!("TIFF sample buffer is truncated"))?);
            }
        }
    }
    Ok(output)
}

fn convert_tiff_color(
    samples: Vec<f32>,
    layout: TiffColorLayout,
    palette: Option<&[u16]>,
    bit_depth: u8,
    pixel_type: PixelType,
) -> Result<(Vec<f32>, PixelType)> {
    let pixels = match layout {
        TiffColorLayout::Direct { .. } => return Ok((samples, pixel_type)),
        TiffColorLayout::Palette => {
            let palette = palette.ok_or_else(|| eyre!("Palette TIFF has no color map"))?;
            let entries = 1_usize
                .checked_shl(bit_depth as u32)
                .ok_or_else(|| eyre!("Invalid TIFF palette depth {bit_depth}"))?;
            if palette.len() < entries * 3 {
                return Err(eyre!("TIFF color map is truncated"));
            }
            let mut output = Vec::with_capacity(samples.len() * 3);
            for sample in samples {
                let index = (sample * (entries - 1) as f32).round() as usize;
                output.extend([
                    palette[index] as f32 / u16::MAX as f32,
                    palette[entries + index] as f32 / u16::MAX as f32,
                    palette[entries * 2 + index] as f32 / u16::MAX as f32,
                ]);
            }
            return Ok((output, PixelType::U16));
        }
        TiffColorLayout::Cmyk { alpha } => {
            let input_channels = 4 + usize::from(alpha);
            let mut output = Vec::with_capacity(samples.len() / input_channels * (3 + usize::from(alpha)));
            for pixel in samples.chunks_exact(input_channels) {
                let k = 1.0 - pixel[3];
                output.extend([(1.0 - pixel[0]) * k, (1.0 - pixel[1]) * k, (1.0 - pixel[2]) * k]);
                if alpha {
                    output.push(pixel[4]);
                }
            }
            output
        }
        TiffColorLayout::YCbCr => {
            let mut output = Vec::with_capacity(samples.len());
            for pixel in samples.chunks_exact(3) {
                let y = pixel[0];
                let cb = pixel[1] - 0.5;
                let cr = pixel[2] - 0.5;
                output.extend([
                    (y + 1.402 * cr).clamp(0.0, 1.0),
                    (y - 0.344_136 * cb - 0.714_136 * cr).clamp(0.0, 1.0),
                    (y + 1.772 * cb).clamp(0.0, 1.0),
                ]);
            }
            output
        }
        TiffColorLayout::Lab => {
            let mut output = Vec::with_capacity(samples.len());
            for pixel in samples.chunks_exact(3) {
                output.extend(tiff_lab_to_rgb(pixel[0], pixel[1], pixel[2]));
            }
            return Ok((output, PixelType::F32));
        }
    };
    Ok((pixels, pixel_type))
}

fn tiff_lab_to_rgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    let l = l * 100.0;
    let a = a * 255.0 - 128.0;
    let b = b * 255.0 - 128.0;
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let pivot = |value: f32| {
        let cube = value * value * value;
        if cube > 0.008_856 {
            cube
        } else {
            (116.0 * value - 16.0) / 903.3
        }
    };
    let x = 0.950_47 * pivot(fx);
    let y = pivot(fy);
    let z = 1.088_83 * pivot(fz);
    let linear = [
        3.240_454 * x - 1.537_139 * y - 0.498_531 * z,
        -0.969_266 * x + 1.876_011 * y + 0.041_556 * z,
        0.055_643 * x - 0.204_026 * y + 1.057_225 * z,
    ];
    linear.map(|value| {
        let value = value.max(0.0);
        if value <= 0.003_130_8 {
            12.92 * value
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        }
        .clamp(0.0, 1.0)
    })
}

fn decoded_dynamic_image(image: DynamicImage) -> Result<DecodedImage> {
    match image {
        DynamicImage::ImageLuma8(image) => {
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 1, PixelType::U8, normalize_u8(image.into_raw()))
        }
        DynamicImage::ImageLumaA8(image) => {
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 2, PixelType::U8, normalize_u8(image.into_raw()))
        }
        DynamicImage::ImageRgb8(image) => {
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 3, PixelType::U8, normalize_u8(image.into_raw()))
        }
        DynamicImage::ImageRgba8(image) => {
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 4, PixelType::U8, normalize_u8(image.into_raw()))
        }
        DynamicImage::ImageLuma16(image) => {
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 1, PixelType::U16, normalize_u16(image.into_raw()))
        }
        DynamicImage::ImageLumaA16(image) => {
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 2, PixelType::U16, normalize_u16(image.into_raw()))
        }
        DynamicImage::ImageRgb16(image) => {
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 3, PixelType::U16, normalize_u16(image.into_raw()))
        }
        DynamicImage::ImageRgba16(image) => {
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 4, PixelType::U16, normalize_u16(image.into_raw()))
        }
        DynamicImage::ImageRgb32F(image) => {
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 3, PixelType::F32, image.into_raw())
        }
        DynamicImage::ImageRgba32F(image) => {
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 4, PixelType::F32, image.into_raw())
        }
        other => {
            let image = other.into_rgba32f();
            let (width, height) = image.dimensions();
            DecodedImage::new(width, height, 4, PixelType::F32, image.into_raw())
        }
    }
}

fn normalize_u8(values: Vec<u8>) -> Vec<f32> {
    values.into_iter().map(|value| value as f32 / u8::MAX as f32).collect()
}

fn normalize_u16(values: Vec<u16>) -> Vec<f32> {
    values.into_iter().map(|value| value as f32 / u16::MAX as f32).collect()
}

pub fn decode_pfm(bytes: &[u8]) -> Result<DecodedImage> {
    let mut offset = 0;
    let magic = next_pfm_token(bytes, &mut offset)?;
    let channels = match magic {
        b"PF" => 3,
        b"Pf" => 1,
        _ => return Err(eyre!("Invalid PFM magic")),
    };
    let width = parse_pfm_token::<u32>(next_pfm_token(bytes, &mut offset)?, "width")?;
    let height = parse_pfm_token::<u32>(next_pfm_token(bytes, &mut offset)?, "height")?;
    let scale = parse_pfm_token::<f32>(next_pfm_token(bytes, &mut offset)?, "scale")?;
    if width == 0 || height == 0 || scale == 0.0 || !scale.is_finite() {
        return Err(eyre!("Invalid PFM dimensions or scale"));
    }

    // The binary payload starts after the line terminator following scale.
    while matches!(bytes.get(offset), Some(b' ' | b'\t')) {
        offset += 1;
    }
    if bytes.get(offset) == Some(&b'\r') {
        offset += 1;
    }
    if bytes.get(offset) == Some(&b'\n') {
        offset += 1;
    } else {
        return Err(eyre!("PFM scale line has no line terminator"));
    }

    let row_values = width as usize * channels as usize;
    let value_count = row_values
        .checked_mul(height as usize)
        .ok_or_else(|| eyre!("PFM image size overflow"))?;
    let byte_count = value_count.checked_mul(4).ok_or_else(|| eyre!("PFM byte size overflow"))?;
    let payload = bytes
        .get(offset..offset + byte_count)
        .ok_or_else(|| eyre!("PFM pixel payload is truncated"))?;
    let little_endian = scale.is_sign_negative();
    let scale = scale.abs();
    let mut pixels = vec![0.0; value_count];

    // PFM stores scanlines from bottom to top.
    for source_y in 0..height as usize {
        let target_y = height as usize - source_y - 1;
        let source_row = &payload[source_y * row_values * 4..(source_y + 1) * row_values * 4];
        let target_row = &mut pixels[target_y * row_values..(target_y + 1) * row_values];
        for (target, value) in target_row.iter_mut().zip(source_row.chunks_exact(4)) {
            let bytes: [u8; 4] = value.try_into().expect("exact f32 PFM chunk");
            *target = if little_endian {
                f32::from_le_bytes(bytes)
            } else {
                f32::from_be_bytes(bytes)
            } * scale;
        }
    }

    DecodedImage::new(width, height, channels, PixelType::F32, pixels)
}

fn next_pfm_token<'a>(bytes: &'a [u8], offset: &mut usize) -> Result<&'a [u8]> {
    loop {
        while bytes.get(*offset).is_some_and(u8::is_ascii_whitespace) {
            *offset += 1;
        }
        if bytes.get(*offset) != Some(&b'#') {
            break;
        }
        while bytes.get(*offset).is_some_and(|byte| *byte != b'\n') {
            *offset += 1;
        }
    }
    let start = *offset;
    while bytes.get(*offset).is_some_and(|byte| !byte.is_ascii_whitespace()) {
        *offset += 1;
    }
    if start == *offset {
        return Err(eyre!("Unexpected end of PFM header"));
    }
    Ok(&bytes[start..*offset])
}

fn parse_pfm_token<T: std::str::FromStr>(token: &[u8], name: &str) -> Result<T> {
    std::str::from_utf8(token)?.parse().map_err(|_| eyre!("Invalid PFM {name}"))
}

pub fn decode_flo(bytes: &[u8]) -> Result<DecodedImage> {
    if bytes.len() < 12 {
        return Err(eyre!(".flo: file too small: {} bytes", bytes.len()));
    }
    let magic = f32::from_le_bytes(bytes[0..4].try_into().expect("flo magic"));
    if magic != 202021.25 {
        return Err(eyre!(".flo: invalid magic: {magic}"));
    }
    let width = i32::from_le_bytes(bytes[4..8].try_into().expect("flo width"));
    let height = i32::from_le_bytes(bytes[8..12].try_into().expect("flo height"));
    if width <= 0 || height <= 0 {
        return Err(eyre!(".flo: invalid dimensions: {width}x{height}"));
    }
    let value_count = width as usize * height as usize * 2;
    let data_bytes = value_count.checked_mul(4).ok_or_else(|| eyre!(".flo data size overflow"))?;
    let payload = bytes
        .get(12..12 + data_bytes)
        .ok_or_else(|| eyre!(".flo pixel payload is truncated"))?;
    let pixels = payload
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().expect("exact flo value")))
        .collect();
    DecodedImage::new(width as u32, height as u32, 2, PixelType::F32, pixels)
}

#[cfg(feature = "heif")]
pub fn decode_heif(path: &Path) -> Result<DecodedImage> {
    use std::ffi::{c_void, CStr};

    fn heif_error(error: libheif_sys::heif_error, context: &str) -> color_eyre::Report {
        let message = if error.message.is_null() {
            "unknown libheif error".into()
        } else {
            // SAFETY: libheif returns a valid NUL-terminated message for the
            // lifetime of the error value.
            unsafe { CStr::from_ptr(error.message) }.to_string_lossy()
        };
        eyre!("{context}: {message}")
    }

    let bytes = std::fs::read(path).map_err(|error| eyre!("Failed to read HEIF bytes: {error}"))?;

    // SAFETY: Every libheif object allocated below is released on each exit
    // path, and `bytes` remains alive while the context reads and decodes it.
    unsafe {
        libheif_sys::heif_init(std::ptr::null_mut());
        let context = libheif_sys::heif_context_alloc();
        if context.is_null() {
            return Err(eyre!("Failed to allocate HEIF context"));
        }
        let error = libheif_sys::heif_context_read_from_memory(
            context,
            bytes.as_ptr() as *mut c_void,
            bytes.len(),
            std::ptr::null(),
        );
        if error.code != libheif_sys::heif_error_code_heif_error_Ok {
            libheif_sys::heif_context_free(context);
            return Err(heif_error(error, "Failed to read HEIF image"));
        }

        let mut handle = std::ptr::null_mut();
        let error = libheif_sys::heif_context_get_primary_image_handle(context, &mut handle);
        if error.code != libheif_sys::heif_error_code_heif_error_Ok {
            libheif_sys::heif_context_free(context);
            return Err(heif_error(error, "Failed to get HEIF image handle"));
        }
        let width = libheif_sys::heif_image_handle_get_width(handle);
        let height = libheif_sys::heif_image_handle_get_height(handle);
        if width <= 0 || height <= 0 {
            libheif_sys::heif_image_handle_release(handle);
            libheif_sys::heif_context_free(context);
            return Err(eyre!("Invalid HEIF dimensions: {width}x{height}"));
        }
        let channels = if libheif_sys::heif_image_handle_has_alpha_channel(handle) != 0 {
            4
        } else {
            3
        };
        let chroma = if channels == 4 {
            libheif_sys::heif_chroma_heif_chroma_interleaved_RGBA
        } else {
            libheif_sys::heif_chroma_heif_chroma_interleaved_RGB
        };

        let mut image = std::ptr::null_mut();
        let options = libheif_sys::heif_decoding_options_alloc();
        let error = libheif_sys::heif_decode_image(
            handle,
            &mut image,
            libheif_sys::heif_colorspace_heif_colorspace_RGB,
            chroma,
            options,
        );
        libheif_sys::heif_decoding_options_free(options);
        if error.code != libheif_sys::heif_error_code_heif_error_Ok {
            libheif_sys::heif_image_handle_release(handle);
            libheif_sys::heif_context_free(context);
            return Err(heif_error(error, "Failed to decode HEIF image"));
        }

        let mut stride = 0;
        let source = libheif_sys::heif_image_get_plane_readonly(
            image,
            libheif_sys::heif_channel_heif_channel_interleaved,
            &mut stride,
        );
        let row_bytes = width as usize * channels as usize;
        if source.is_null() || stride < 0 || (stride as usize) < row_bytes {
            libheif_sys::heif_image_release(image);
            libheif_sys::heif_image_handle_release(handle);
            libheif_sys::heif_context_free(context);
            return Err(eyre!("Invalid HEIF image plane or stride"));
        }

        let mut pixels = Vec::with_capacity(row_bytes * height as usize);
        for y in 0..height as usize {
            // SAFETY: stride and visible row length were validated above and
            // libheif owns this plane until `heif_image_release` below.
            let row = std::slice::from_raw_parts(source.add(y * stride as usize), row_bytes);
            pixels.extend(row.iter().map(|&value| value as f32 / u8::MAX as f32));
        }
        libheif_sys::heif_image_release(image);
        libheif_sys::heif_image_handle_release(handle);
        libheif_sys::heif_context_free(context);
        DecodedImage::new(width as u32, height as u32, channels, PixelType::U8, pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exr::prelude::{Image as ExrImage, SpecificChannels, WritableImage};
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};

    fn encode_mono_exr(channel_name: &'static str) -> Vec<u8> {
        let channels = SpecificChannels::build()
            .with_channel::<f32>(channel_name)
            .with_pixel_fn(|position| ([0.25_f32, 0.75][position.x()],));
        let image = ExrImage::from_channels((2, 1), channels);
        let mut bytes = Cursor::new(Vec::new());
        image.write().to_buffered(&mut bytes).unwrap();
        bytes.into_inner()
    }

    fn encode_rgb_exr(with_alpha: bool) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        if with_alpha {
            let channels = SpecificChannels::rgba(|position: Vec2<usize>| {
                (position.x() as f32, position.y() as f32, 0.5_f32, 0.25_f32)
            });
            ExrImage::from_channels((2, 2), channels)
                .write()
                .to_buffered(&mut bytes)
                .unwrap();
        } else {
            let channels =
                SpecificChannels::rgb(|position: Vec2<usize>| (position.x() as f32, position.y() as f32, 0.5_f32));
            ExrImage::from_channels((2, 2), channels)
                .write()
                .to_buffered(&mut bytes)
                .unwrap();
        }
        bytes.into_inner()
    }

    #[test]
    fn decodes_little_endian_pfm_and_flips_rows() {
        let mut bytes = b"Pf\n2 2\n-2.0  \n".to_vec();
        for value in [3.0_f32, 4.0, 1.0, 2.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let image = decode_pfm(&bytes).unwrap();
        assert_eq!((image.width, image.height, image.channels), (2, 2, 1));
        assert_eq!(image.pixels, vec![2.0, 4.0, 6.0, 8.0]);
    }

    #[test]
    fn decodes_middlebury_flo() {
        let mut bytes = 202021.25_f32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&2_i32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        for value in [1.0_f32, -1.0, 2.0, -2.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let image = decode_flo(&bytes).unwrap();
        assert_eq!((image.width, image.height, image.channels), (2, 1, 2));
        assert_eq!(image.pixels, vec![1.0, -1.0, 2.0, -2.0]);
    }

    #[test]
    fn decodes_rgb_and_rgba_exr_through_the_dedicated_path() {
        for (with_alpha, expected_channels) in [(false, 3), (true, 4)] {
            let decoded = decode_bytes(&encode_rgb_exr(with_alpha)).unwrap();
            assert_eq!((decoded.width, decoded.height), (2, 2));
            assert_eq!(decoded.channels, expected_channels);
            assert_eq!(decoded.pixel_type, PixelType::F32);
            assert_eq!(decoded.pixels.len(), 2 * 2 * expected_channels as usize);
        }
    }

    #[test]
    fn decodes_r_l_y_and_z_exr_as_one_float_channel() {
        for channel_name in ["R", "L", "Y", "Z"] {
            let decoded = decode_bytes(&encode_mono_exr(channel_name)).unwrap();
            assert_eq!((decoded.width, decoded.height, decoded.channels), (2, 1, 1), "{channel_name}");
            assert_eq!(decoded.pixel_type, PixelType::F32, "{channel_name}");
            assert_eq!(decoded.pixels, vec![0.25, 0.75], "{channel_name}");
        }
    }

    #[test]
    fn dedicated_tiff_path_decodes_signed_and_float_gray_samples() {
        use tiff::encoder::{colortype, TiffEncoder};

        let mut signed = Cursor::new(Vec::new());
        TiffEncoder::new(&mut signed)
            .unwrap()
            .write_image::<colortype::GrayI32>(3, 1, &[-i32::MAX, 0, i32::MAX])
            .unwrap();
        let decoded = decode_bytes(signed.get_ref()).unwrap();
        assert_eq!((decoded.width, decoded.height, decoded.channels), (3, 1, 1));
        assert_eq!(decoded.pixel_type, PixelType::I32);
        assert_eq!(decoded.pixels, vec![-1.0, 0.0, 1.0]);

        let mut float = Cursor::new(Vec::new());
        TiffEncoder::new(&mut float)
            .unwrap()
            .write_image::<colortype::Gray32Float>(3, 1, &[-0.25, 0.5, 2.0])
            .unwrap();
        let decoded = decode_bytes(float.get_ref()).unwrap();
        assert_eq!((decoded.width, decoded.height, decoded.channels), (3, 1, 1));
        assert_eq!(decoded.pixel_type, PixelType::F32);
        assert_eq!(decoded.pixels, vec![-0.25, 0.5, 2.0]);
    }

    #[test]
    fn image_rs_round_trips_enabled_ldr_formats() {
        let source = DynamicImage::ImageRgb8(ImageBuffer::from_fn(3, 2, |x, y| {
            Rgb([(x * 70) as u8, (y * 110) as u8, ((x + y) * 40) as u8])
        }));
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Bmp,
            ImageFormat::Tiff,
            ImageFormat::WebP,
            ImageFormat::Pnm,
            ImageFormat::Gif,
            ImageFormat::Tga,
            ImageFormat::Qoi,
        ] {
            let mut encoded = Cursor::new(Vec::new());
            source.write_to(&mut encoded, format).unwrap();
            let decoded = decode_bytes(encoded.get_ref()).unwrap();
            assert_eq!((decoded.width, decoded.height), (3, 2), "{format:?}");
            assert!(matches!(decoded.channels, 3 | 4), "{format:?}");
        }

        let rgba = DynamicImage::ImageRgba8(source.to_rgba8());
        let mut encoded = Cursor::new(Vec::new());
        rgba.write_to(&mut encoded, ImageFormat::Ico).unwrap();
        let decoded = decode_bytes(encoded.get_ref()).unwrap();
        assert_eq!((decoded.width, decoded.height, decoded.channels), (3, 2, 4));

        let rgba16 = DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(3, 2, |x, y| {
            image::Rgba([(x * 10_000) as u16, (y * 20_000) as u16, 30_000, u16::MAX])
        }));
        let mut encoded = Cursor::new(Vec::new());
        rgba16.write_to(&mut encoded, ImageFormat::Farbfeld).unwrap();
        let decoded = decode_bytes(encoded.get_ref()).unwrap();
        assert_eq!((decoded.width, decoded.height, decoded.channels), (3, 2, 4));
        assert_eq!(decoded.pixel_type, PixelType::U16);
    }
}
