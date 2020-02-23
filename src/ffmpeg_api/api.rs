use ffmpeg_dev::sys as ffi;
use failure::bail;
use enum_primitive::*;
use std::marker::PhantomData;
use fraction::Fraction;

use crate::ffmpeg_api::enums::*;

// TODO: Use proper errors (with struct etc) for this
enum_from_primitive! {
    #[derive(Debug, Copy, Clone, PartialEq)]
    #[repr(i32)]
    pub enum AVErrorKind {
        Unknown = ffi::AVERROR_EXPERIMENTAL,
        InputChanged = ffi::AVERROR_INPUT_CHANGED,
        OutputChanged = ffi::AVERROR_OUTPUT_CHANGED
    }
}

pub struct AVFormatContext {
    base: *mut ffi::AVFormatContext,
}

impl<'a> AVFormatContext {
    pub fn new() -> Result<Self, failure::Error> {
        let base = unsafe { ffi::avformat_alloc_context() };
        if base.is_null() {
            bail!("avformat_alloc_context() failed");
        }
        Ok(AVFormatContext { base })
    }

    // TODO: Just for testing
    pub unsafe fn raw(&self) -> *mut ffi::AVFormatContext {
        self.base
    }

    pub fn open_input(&mut self, path: &str) -> Result<(), failure::Error> {
        match unsafe {
            ffi::avformat_open_input(
                &mut self.base,
                std::ffi::CString::new(path)
                    .map_err(|_| failure::format_err!("Could not convert path to c string"))?
                    .as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } {
            0 => Ok(()),
            _ => bail!("Could not open input")
        }
    }

    pub fn streams(&self) -> Vec<AVStream> {
        return unsafe {
            std::slice::from_raw_parts(
                (*self.base).streams,
                (*self.base).nb_streams as usize,
            )
        }
            .iter()
            .map(|stream| {
                AVStream::new(unsafe { (*stream).as_mut() }.expect("not null"), self)
            })
            .collect();
    }
}

impl Drop for AVFormatContext {
    fn drop(&mut self) {
        unsafe { ffi::avformat_free_context(self.base) }
    }
}

pub struct AVBuffer {
    base: *mut u8,
    size: usize,
}

impl AVBuffer {
    pub fn new(size: usize) -> Result<Self, failure::Error> {
        let base = unsafe { ffi::av_malloc(size) } as *mut u8;
        if base.is_null() {
            bail!("av_malloc() failed");
        }
        Ok(AVBuffer { base, size })
    }

    pub fn empty() -> Self {
        AVBuffer { base: std::ptr::null_mut(), size: 0 }
    }

    pub fn data(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(self.base, self.size)
        }
    }

    pub fn data_mut(&mut self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts_mut(self.base, self.size)
        }
    }
}

pub struct AVFrame {
    base: *mut ffi::AVFrame,
    buffer: AVBuffer,
}

impl AVFrame {
    pub fn new() -> Result<Self, failure::Error> {
        let base = unsafe { ffi::av_frame_alloc() };
        if base.is_null() {
            bail!("avformat_alloc_frame() failed");
        }
        Ok(AVFrame { base, buffer: AVBuffer::empty() })
    }

    // TODO: Just for testing
    pub unsafe fn as_mut(&mut self) -> &mut ffi::AVFrame {
        self.base.as_mut().expect("not null")
    }

    pub fn init(&mut self, width: i32, height: i32, format: AVPixelFormat) -> Result<(), failure::Error>{
        let mut base = unsafe { self.base.as_mut() }.expect("not null");

        base.width = width;
        base.height = height;
        base.format = format as ffi::AVPixelFormat;

        self.buffer = AVBuffer::new(self.size())?;

        unsafe {
            ffi::avpicture_fill(
                self.base as *mut ffi::AVPicture,
                self.buffer.base as *mut u8,
                self.format() as ffi::AVPixelFormat,
                self.width(),
                self.height(),
            )
        };

        Ok(())
    }

    pub fn width(&self) -> i32 {
        let base = unsafe { self.base.as_ref() }.expect("not null");

        base.width
    }

    pub fn height(&self) -> i32 {
        let base = unsafe { self.base.as_ref() }.expect("not null");

        base.height
    }

    pub fn format(&self) -> AVPixelFormat {
        let base = unsafe { self.base.as_ref() }.expect("not null");

        AVPixelFormat::from_i32(base.format)
            .unwrap_or(AVPixelFormat::NONE)
    }

    pub fn size(&self) -> usize {
        unsafe {
            ffi::avpicture_get_size(self.format() as ffi::AVPixelFormat, self.width(), self.height()) as usize
        }
    }

    pub fn key_frame(&self) -> bool {
        let base = unsafe { self.base.as_ref() }.expect("not null");

        base.key_frame != 0
    }

    pub fn pts(&self) -> i64 {
        let base = unsafe { self.base.as_ref() }.expect("not null");

        base.pts
    }

    pub fn coded_picture_number(&self) -> i32 {
        let base = unsafe { self.base.as_ref() }.expect("not null");

        base.coded_picture_number
    }

    pub fn display_picture_number(&self) -> i32 {
        let base = unsafe { self.base.as_ref() }.expect("not null");

        base.display_picture_number
    }

    pub fn linesize(&self) -> &[i32] {
        let base = unsafe { self.base.as_ref() }.expect("not null");

        &base.linesize
    }

    pub fn data_ptr(&self) -> *const *const u8 {
        let base = unsafe { self.base.as_ref() }.expect("not null");

        base.data.as_ptr() as *const *const u8
    }

    pub fn data_mut_ptr(&mut self) -> *mut *mut u8 {
        let base = unsafe { self.base.as_mut() }.expect("not null");

        base.data.as_mut_ptr() as *mut *mut u8
    }

    pub fn data(&self, index: usize) -> &[u8] {
        let base = unsafe { self.base.as_ref() }.expect("not null");

        unsafe {
            std::slice::from_raw_parts(base.data[index], self.size())
        }
    }

    pub fn data_mut(&mut self, index: usize) -> &mut [u8] {
        let base = unsafe { self.base.as_mut() }.expect("not null");

        unsafe {
            std::slice::from_raw_parts_mut(base.data[index], self.size())
        }
    }
}

impl Drop for AVFrame {
    fn drop(&mut self) {
        unsafe { ffi::av_frame_free(&mut self.base) }
    }
}

pub struct AVStream<'a> {
    base: &'a mut ffi::AVStream,
    phantom: PhantomData<&'a AVFormatContext>,
}

impl<'a> AVStream<'a> {
    fn new(base: &'a mut ffi::AVStream, _: &'a AVFormatContext) -> Self {
        return AVStream { base, phantom: PhantomData };
    }

    pub fn index(self: &AVStream<'a>) -> i32 {
        self.base.index
    }

    pub fn time_base(self: &AVStream<'a>) -> Fraction {
        Fraction::new(
            self.base.time_base.num as u32,
            self.base.time_base.den as u32,
        )
    }

    pub fn timestamp(self: &AVStream<'a>, timestamp: i64) -> std::time::Duration {
        std::time::Duration::from_millis(
            1000 *
                timestamp as u64 *
                self.base.time_base.num as u64 /
                self.base.time_base.den as u64
        )
    }

    pub fn duration(self: &AVStream<'a>) -> std::time::Duration {
        self.timestamp(self.base.duration)
    }

    pub fn frame_count(self: &AVStream<'a>) -> i64 {
        self.base.nb_frames
    }

    pub fn discard(self: &AVStream<'a>) -> Option<AVDiscard> {
        AVDiscard::from_i32(self.base.discard)
    }

    pub fn set_discard(self: &mut AVStream<'a>, value: AVDiscard) {
        self.base.discard = value as ffi::AVDiscard;
    }

    pub fn sample_aspect_ratio(self: &AVStream<'a>) -> Fraction {
        Fraction::new(
            self.base.sample_aspect_ratio.num as u32,
            self.base.sample_aspect_ratio.den as u32,
        )
    }

    pub fn codec_parameters(self: &AVStream<'a>) -> AVCodecParameters {
        AVCodecParameters::new(unsafe { self.base.codecpar.as_mut() }.expect("not null"), self)
    }
}

pub struct AVCodecParameters<'a> {
    base: &'a mut ffi::AVCodecParameters,
    phantom: PhantomData<&'a AVStream<'a>>,
}

impl<'a> AVCodecParameters<'a> {
    fn new(base: &'a mut ffi::AVCodecParameters, _: &'a AVStream) -> Self {
        return AVCodecParameters { base, phantom: PhantomData };
    }

    // TODO: Just for testing
    pub unsafe fn as_ref(&self) -> &ffi::AVCodecParameters {
        self.base
    }

    pub fn codec_type(self: &AVCodecParameters<'a>) -> AVMediaType {
        AVMediaType::from_i32(self.base.codec_type).unwrap_or(AVMediaType::Unknown)
    }

    pub fn codec_id(self: &AVCodecParameters<'a>) -> Option<AVCodecID> {
        AVCodecID::from_u32(self.base.codec_id)
    }

    pub fn find_decoder(self: &AVCodecParameters<'a>) -> AVCodec {
        AVCodec::new(
            unsafe { ffi::avcodec_find_decoder(self.base.codec_id).as_mut() }.expect("Decoder not found"),
            self,
        )
    }
}

pub struct AVCodec<'a> {
    base: &'a mut ffi::AVCodec,
    phantom: PhantomData<&'a AVCodecParameters<'a>>,
}

impl<'a> AVCodec<'a> {
    fn new(base: &'a mut ffi::AVCodec, _: &'a AVCodecParameters) -> Self {
        return AVCodec { base, phantom: PhantomData };
    }

    // TODO: Just for testing
    pub unsafe fn as_ref(&self) -> &ffi::AVCodec {
        self.base
    }

    pub fn name(self: &AVCodec<'a>) -> std::string::String {
        String::from(unsafe { std::ffi::CStr::from_ptr(self.base.name) }.to_str().unwrap())
    }
}
