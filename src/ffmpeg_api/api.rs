use std::marker::PhantomData;
use std::path::Path;

use enum_primitive::*;
use failure::{bail, format_err, Error};
use ffmpeg_dev::sys as ffi;
use fraction::Fraction;

use crate::ffmpeg_api::enums::*;
use crate::util::media_time;

fn native_string(ptr: *const std::os::raw::c_char) -> Result<String, Error> {
    if ptr.is_null() {
        Err(format_err!("String is null"))
    } else {
        Ok(String::from(
            unsafe { std::ffi::CStr::from_ptr(ptr) }
                .to_str()
                .map_err(|err| format_err!("String is not valid utf-8: {}", err))?,
        ))
    }
}

pub struct AVFormatContext {
    base: *mut ffi::AVFormatContext,
}

impl AVFormatContext {
    pub fn new() -> Result<Self, Error> {
        let base = unsafe { ffi::avformat_alloc_context() };
        if base.is_null() {
            bail!("avformat_alloc_context() failed");
        }
        Ok(AVFormatContext { base })
    }

    pub fn open_input(&mut self, path: &Path) -> Result<(), Error> {
        let path = path
            .to_str()
            .ok_or(format_err!("Could not convert path to c string"))?;
        let path = std::ffi::CString::new(path)
            .map_err(|err| format_err!("Could not convert path to c string: {}", err))?;

        match unsafe {
            ffi::avformat_open_input(
                &mut self.base,
                path.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } {
            0 => Ok(()),
            _ => bail!("Could not open input"),
        }
    }

    pub fn input_format(&self) -> Result<AVInputFormat, Error> {
        let base: &mut ffi::AVInputFormat = unsafe { (*self.base).iformat.as_mut() }
            .ok_or(format_err!("No AVInputFormat found"))?;

        Ok(AVInputFormat::new(base))
    }

    pub fn streams(&self) -> Vec<AVStream> {
        Vec::from(unsafe {
            std::slice::from_raw_parts((*self.base).streams, (*self.base).nb_streams as usize)
        })
        .iter()
        .filter_map(|stream: &*mut ffi::AVStream| unsafe { (*stream).as_mut() })
        .map(|stream| AVStream::new(stream))
        .collect()
    }

    pub fn find_stream<P>(&self, predicate: P) -> Option<AVStream>
    where
        P: FnMut(&AVStream) -> bool,
    {
        Vec::from(unsafe {
            std::slice::from_raw_parts((*self.base).streams, (*self.base).nb_streams as usize)
        })
        .iter()
        .filter_map(|stream: &*mut ffi::AVStream| unsafe { (*stream).as_mut() })
        .map(|stream| AVStream::new(stream))
        .find(predicate)
    }

    pub fn read_frame(&mut self, packet: &mut AVPacket) -> Result<(), Error> {
        match unsafe { ffi::av_read_frame(self.base, packet.base) } {
            0 => Ok(()),
            errno => bail!("Error while decoding frame: {}", errno),
        }
    }

    pub fn duration(&self) -> Result<media_time::MediaTime, Error> {
        media_time::MediaTime::from_rational(
            unsafe { (*self.base).duration },
            Fraction::new(1 as u64, ffi::AV_TIME_BASE as u64),
        )
    }
}

impl Drop for AVFormatContext {
    fn drop(&mut self) {
        unsafe { ffi::avformat_free_context(self.base) }
    }
}

pub struct AVInputFormat<'a> {
    base: &'a mut ffi::AVInputFormat,
}

impl<'a> AVInputFormat<'a> {
    fn new(base: &'a mut ffi::AVInputFormat) -> Self {
        return AVInputFormat { base };
    }

    pub fn long_name(&self) -> Result<String, Error> {
        native_string(self.base.long_name)
            .map_err(|err| format_err!("Could not access long name for input format: {}", err))
    }

    pub fn name(&self) -> Result<String, Error> {
        native_string(self.base.name)
            .map_err(|err| format_err!("Could not access short name for input format: {}", err))
    }

    pub fn determine_mime(&self, stream_codec: impl AsRef<str>) -> Result<&str, Error> {
        let containers = self.name()?;
        let stream_codec = stream_codec.as_ref();

        for container in containers.split(",") {
            match (container, stream_codec) {
                ("mp4", "h264") | ("mp4", "hevc") => return Ok("video/mp4"),
                ("matroska", "h264") | ("matroska", "hevc") => return Ok("video/x-matroska"),
                ("webm", "vp8") | ("webm", "vp9") | ("webm", "av1") => return Ok("video/webm"),
                _ => {}
            }
        }

        bail!(
            "Could not determine mime type: {} video in {} container",
            stream_codec,
            containers
        )
    }
}

pub struct AVBuffer {
    base: *mut u8,
    size: usize,
}

impl AVBuffer {
    pub fn new(size: usize) -> Result<Self, Error> {
        let base = unsafe { ffi::av_malloc(size) } as *mut u8;
        if base.is_null() {
            bail!("av_malloc() failed");
        }
        Ok(AVBuffer { base, size })
    }

    pub fn empty() -> Self {
        AVBuffer {
            base: std::ptr::null_mut(),
            size: 0,
        }
    }

    pub fn data(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.base, self.size) }
    }

    pub fn data_mut(&mut self) -> &[u8] {
        unsafe { std::slice::from_raw_parts_mut(self.base, self.size) }
    }
}

pub struct AVPacket {
    base: *mut ffi::AVPacket,
}

impl AVPacket {
    pub fn new() -> Result<Self, Error> {
        let base = unsafe { ffi::av_packet_alloc() };
        if base.is_null() {
            bail!("av_packet_alloc() failed");
        }
        Ok(AVPacket { base })
    }

    fn as_ref(&self) -> &ffi::AVPacket {
        unsafe { self.base.as_ref() }.unwrap_or_else(|| panic!("AVPacket base unexpectedly null"))
    }

    pub fn pts(&self) -> i64 {
        self.as_ref().pts
    }

    pub fn dts(&self) -> i64 {
        self.as_ref().dts
    }

    pub fn stream_index(&self) -> i32 {
        self.as_ref().stream_index
    }
}

impl Drop for AVPacket {
    fn drop(&mut self) {
        unsafe { ffi::av_packet_free(&mut self.base) }
    }
}

pub struct AVFrame {
    base: *mut ffi::AVFrame,
    buffer: AVBuffer,
}

impl AVFrame {
    pub fn new() -> Result<Self, Error> {
        let base = unsafe { ffi::av_frame_alloc() };
        if base.is_null() {
            bail!("avformat_alloc_frame() failed");
        }
        Ok(AVFrame {
            base,
            buffer: AVBuffer::empty(),
        })
    }

    pub fn init(&mut self, width: i32, height: i32, format: AVPixelFormat) -> Result<(), Error> {
        self.as_mut().width = width;
        self.as_mut().height = height;
        self.as_mut().format = format as ffi::AVPixelFormat;

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

    fn as_ref(&self) -> &ffi::AVFrame {
        unsafe { self.base.as_ref() }.unwrap_or_else(|| panic!("AVFrame base unexpectedly null"))
    }

    fn as_mut(&mut self) -> &mut ffi::AVFrame {
        unsafe { self.base.as_mut() }.unwrap_or_else(|| panic!("AVFrame base unexpectedly null"))
    }

    pub fn width(&self) -> i32 {
        self.as_ref().width
    }

    pub fn height(&self) -> i32 {
        self.as_ref().height
    }

    pub fn format(&self) -> AVPixelFormat {
        AVPixelFormat::from_i32(self.as_ref().format).unwrap_or(AVPixelFormat::NONE)
    }

    pub fn size(&self) -> usize {
        unsafe {
            ffi::avpicture_get_size(
                self.format() as ffi::AVPixelFormat,
                self.width(),
                self.height(),
            ) as usize
        }
    }

    pub fn key_frame(&self) -> bool {
        self.as_ref().key_frame != 0
    }

    pub fn pts(&self) -> i64 {
        self.as_ref().pts
    }

    pub fn coded_picture_number(&self) -> i32 {
        self.as_ref().coded_picture_number
    }

    pub fn display_picture_number(&self) -> i32 {
        self.as_ref().display_picture_number
    }

    pub fn linesize(&self) -> &[i32] {
        &self.as_ref().linesize
    }

    pub fn data_ptr(&self) -> *const *const u8 {
        self.as_ref().data.as_ptr() as *const *const u8
    }

    pub fn data_mut_ptr(&mut self) -> *mut *mut u8 {
        self.as_mut().data.as_mut_ptr() as *mut *mut u8
    }

    pub fn data(&self, index: usize) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.as_ref().data[index], self.size()) }
    }

    pub fn data_mut(&mut self, index: usize) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.as_mut().data[index], self.size()) }
    }
}

impl Drop for AVFrame {
    fn drop(&mut self) {
        unsafe { ffi::av_frame_free(&mut self.base) }
    }
}

pub struct AVStream<'a> {
    base: &'a mut ffi::AVStream,
}

impl<'a> AVStream<'a> {
    fn new(base: &'a mut ffi::AVStream) -> Self {
        return AVStream { base };
    }

    pub fn index(&self) -> i32 {
        self.base.index
    }

    pub fn time_base(&self) -> Fraction {
        Fraction::new(
            self.base.time_base.num as u32,
            self.base.time_base.den as u32,
        )
    }

    pub fn timestamp(&self, timestamp: i64) -> Result<media_time::MediaTime, Error> {
        media_time::MediaTime::from_rational(timestamp, self.time_base())
    }

    pub fn duration(&self) -> Result<media_time::MediaTime, Error> {
        self.timestamp(self.base.duration)
    }

    pub fn frame_count(&self) -> i64 {
        self.base.nb_frames
    }

    pub fn discard(&self) -> Option<AVDiscard> {
        AVDiscard::from_i32(self.base.discard)
    }

    pub fn set_discard(&mut self, value: AVDiscard) {
        self.base.discard = value as ffi::AVDiscard;
    }

    pub fn sample_aspect_ratio(&self) -> Fraction {
        Fraction::new(
            self.base.sample_aspect_ratio.num as u32,
            self.base.sample_aspect_ratio.den as u32,
        )
    }

    pub fn display_aspect_ratio(&self) -> Fraction {
        Fraction::new(
            self.base.display_aspect_ratio.num as u32,
            self.base.display_aspect_ratio.den as u32,
        )
    }

    pub fn codec_parameters(&self) -> Result<AVCodecParameters, Error> {
        Ok(AVCodecParameters::new(
            unsafe { self.base.codecpar.as_mut() }
                .ok_or(format_err!("No AVCodecParameters found"))?,
            self,
        ))
    }
}

pub struct AVCodecParameters<'a> {
    base: &'a mut ffi::AVCodecParameters,
    phantom: PhantomData<&'a AVStream<'a>>,
}

impl<'a> AVCodecParameters<'a> {
    fn new(base: &'a mut ffi::AVCodecParameters, _: &'a AVStream) -> Self {
        return AVCodecParameters {
            base,
            phantom: PhantomData,
        };
    }

    pub fn codec_type(&self) -> AVMediaType {
        AVMediaType::from_i32(self.base.codec_type).unwrap_or(AVMediaType::Unknown)
    }

    pub fn codec_id(&self) -> Option<AVCodecID> {
        AVCodecID::from_u32(self.base.codec_id)
    }

    pub fn bit_rate(&self) -> i64 {
        self.base.bit_rate
    }

    pub fn find_decoder(&self) -> Result<AVCodec, Error> {
        Ok(AVCodec::new(
            unsafe { ffi::avcodec_find_decoder(self.base.codec_id).as_mut() }
                .ok_or(format_err!("No AVCodec found"))?,
            self,
        ))
    }
}

pub struct AVCodec<'a> {
    base: &'a mut ffi::AVCodec,
    phantom: PhantomData<&'a AVCodecParameters<'a>>,
}

impl<'a> AVCodec<'a> {
    fn new(base: &'a mut ffi::AVCodec, _: &'a AVCodecParameters) -> Self {
        return AVCodec {
            base,
            phantom: PhantomData,
        };
    }

    pub fn name(&self) -> Result<String, Error> {
        native_string(self.base.name)
            .map_err(|err| format_err!("Could not access name for codec: {}", err))
    }
}

pub struct AVCodecContext {
    base: *mut ffi::AVCodecContext,
}

impl AVCodecContext {
    pub fn new(codec: &AVCodec) -> Result<Self, Error> {
        let base = unsafe { ffi::avcodec_alloc_context3(codec.base) };
        if base.is_null() {
            bail!("avcodec_alloc_context3() failed");
        }
        Ok(AVCodecContext { base })
    }

    pub fn in_packet(&mut self, packet: &mut AVPacket) -> Result<(), Error> {
        match unsafe { ffi::avcodec_send_packet(self.base, packet.base) } {
            0 => Ok(()),
            errno => Err(format_err!("Error while loading packet: {}", errno)),
        }
    }

    pub fn out_frame(&mut self, frame: &mut AVFrame) -> Result<(), Error> {
        match unsafe { ffi::avcodec_receive_frame(self.base, frame.base) } {
            0 => Ok(()),
            errno => Err(format_err!("Error while decoding frame: {}", errno)),
        }
    }

    fn as_ref(&self) -> &ffi::AVCodecContext {
        unsafe { self.base.as_ref() }
            .unwrap_or_else(|| panic!("AVCodecContext base unexpectedly null"))
    }

    fn as_mut(&mut self) -> &mut ffi::AVCodecContext {
        unsafe { self.base.as_mut() }
            .unwrap_or_else(|| panic!("AVCodecContext base unexpectedly null"))
    }

    pub fn skip_loop_filter(&self) -> Option<AVDiscard> {
        AVDiscard::from_i32(self.as_ref().skip_loop_filter)
    }

    pub fn set_skip_loop_filter(&mut self, value: AVDiscard) {
        self.as_mut().skip_loop_filter = value as ffi::AVDiscard
    }

    pub fn skip_idct(&self) -> Option<AVDiscard> {
        AVDiscard::from_i32(self.as_ref().skip_idct)
    }

    pub fn set_skip_idct(&mut self, value: AVDiscard) {
        self.as_mut().skip_idct = value as ffi::AVDiscard
    }

    pub fn skip_frame(&self) -> Option<AVDiscard> {
        AVDiscard::from_i32(self.as_ref().skip_frame)
    }

    pub fn set_skip_frame(&mut self, value: AVDiscard) {
        self.as_mut().skip_frame = value as ffi::AVDiscard
    }

    pub fn set_parameters(&mut self, params: &AVCodecParameters) {
        unsafe {
            ffi::avcodec_parameters_to_context(self.base, params.base);
        }
    }

    pub fn open(&mut self, codec: &AVCodec) {
        unsafe {
            ffi::avcodec_open2(self.base, codec.base, std::ptr::null_mut());
        }
    }
}

impl Drop for AVCodecContext {
    fn drop(&mut self) {
        unsafe { ffi::avcodec_free_context(&mut self.base) }
    }
}

pub struct SwsContext {
    base: *mut ffi::SwsContext,
}

impl SwsContext {
    pub fn new() -> Self {
        SwsContext {
            base: std::ptr::null_mut(),
        }
    }

    pub fn reinit(
        &mut self,
        source: &AVFrame,
        target: &AVFrame,
        scaler: SwsScaler,
        flags: SwsFlags,
    ) -> Result<(), Error> {
        let base = unsafe {
            ffi::sws_getCachedContext(
                self.base,
                source.width(),
                source.height(),
                source.format() as ffi::AVPixelFormat,
                target.width(),
                target.height(),
                target.format() as ffi::AVPixelFormat,
                scaler as std::os::raw::c_int | flags.bits() as std::os::raw::c_int,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if base.is_null() {
            bail!("sws_getCachedContext() failed");
        }
        self.base = base;

        Ok(())
    }

    pub fn scale(&self, source: &AVFrame, target: &mut AVFrame) -> i32 {
        self.scale_slice(source, target, 0, source.height())
    }

    pub fn scale_slice(
        &self,
        source: &AVFrame,
        target: &mut AVFrame,
        slice_from: i32,
        slice_to: i32,
    ) -> i32 {
        unsafe {
            ffi::sws_scale(
                self.base,
                source.data_ptr(),
                source.linesize().as_ptr(),
                slice_from,
                slice_to,
                target.data_mut_ptr(),
                target.linesize().as_ptr(),
            )
        }
    }
}

impl Drop for SwsContext {
    fn drop(&mut self) {
        unsafe { ffi::sws_freeContext(self.base) }
    }
}
