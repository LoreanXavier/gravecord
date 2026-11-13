//! A small TensorFlow Lite runner for the person-segmentation model the official engine ships
//! (`selfie_segmentation.tflite` and `selfie_segmentation_landscape.tflite` next to `discord_voice.node`, MediaPipe
//! Selfie Segmentation, Apache-2.0).
//!
//! It reads the flatbuffer itself and runs exactly the operators those two graphs hold, in float32 on the CPU:
//! DEQUANTIZE (float16 weights, folded at load), CONV_2D, DEPTHWISE_CONV_2D, ADD, MUL, RELU, HARD_SWISH, LOGISTIC,
//! MEAN, AVERAGE_POOL_2D, RESIZE_BILINEAR and MediaPipe's custom `Convolution2DTransposeBias`. Any other operator,
//! a quantised tensor or a malformed file is refused at load, so a model that loads runs. The operator semantics are
//! TensorFlow Lite's reference kernels (NHWC, OHWI filters, SAME padding split low-first).

use std::fmt;

/// Why a model file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError(pub String);

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ModelError {}

type Result<T> = std::result::Result<T, ModelError>;

fn bad(message: impl Into<String>) -> ModelError {
    ModelError(message.into())
}

// ---- flatbuffer reading ----

#[derive(Clone, Copy)]
struct Fb<'a>(&'a [u8]);

impl<'a> Fb<'a> {
    fn bytes(self, at: usize, len: usize) -> Result<&'a [u8]> {
        at.checked_add(len)
            .and_then(|end| self.0.get(at..end))
            .ok_or_else(|| bad("model file is truncated"))
    }

    fn u16(self, at: usize) -> Result<u16> {
        let b = self.bytes(at, 2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(self, at: usize) -> Result<u32> {
        let b = self.bytes(at, 4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(self, at: usize) -> Result<i32> {
        Ok(self.u32(at)? as i32)
    }

    /// The position an unsigned offset stored at `at` points to.
    fn follow(self, at: usize) -> Result<usize> {
        at.checked_add(self.u32(at)? as usize)
            .filter(|&target| target < self.0.len())
            .ok_or_else(|| bad("model file has an offset out of range"))
    }

    fn table(self, at: usize) -> Result<Table<'a>> {
        let vtable = (at as i64 - i64::from(self.i32(at)?)) as usize;
        let vtable_len = self.u16(vtable)? as usize;
        Ok(Table {
            fb: self,
            at,
            vtable,
            vtable_len,
        })
    }
}

#[derive(Clone, Copy)]
struct Table<'a> {
    fb: Fb<'a>,
    at: usize,
    vtable: usize,
    vtable_len: usize,
}

impl<'a> Table<'a> {
    fn field(&self, index: usize) -> Result<Option<usize>> {
        let slot = 4 + 2 * index;
        if slot + 2 > self.vtable_len {
            return Ok(None);
        }
        let offset = self.fb.u16(self.vtable + slot)? as usize;
        Ok((offset != 0).then_some(self.at + offset))
    }

    fn u8_or(&self, index: usize, default: u8) -> Result<u8> {
        match self.field(index)? {
            Some(at) => Ok(self.fb.bytes(at, 1)?[0]),
            None => Ok(default),
        }
    }

    fn i32_or(&self, index: usize, default: i32) -> Result<i32> {
        match self.field(index)? {
            Some(at) => self.fb.i32(at),
            None => Ok(default),
        }
    }

    fn u32_or(&self, index: usize, default: u32) -> Result<u32> {
        match self.field(index)? {
            Some(at) => self.fb.u32(at),
            None => Ok(default),
        }
    }

    fn table(&self, index: usize) -> Result<Option<Table<'a>>> {
        match self.field(index)? {
            Some(at) => Ok(Some(self.fb.table(self.fb.follow(at)?)?)),
            None => Ok(None),
        }
    }

    /// A vector's first element position and length.
    fn vector(&self, index: usize) -> Result<Option<(usize, usize)>> {
        match self.field(index)? {
            Some(at) => {
                let start = self.fb.follow(at)?;
                let len = self.fb.u32(start)? as usize;
                Ok(Some((start + 4, len)))
            }
            None => Ok(None),
        }
    }

    fn bytes(&self, index: usize) -> Result<&'a [u8]> {
        match self.vector(index)? {
            Some((start, len)) => self.fb.bytes(start, len),
            None => Ok(&[]),
        }
    }

    fn ints(&self, index: usize) -> Result<Vec<i32>> {
        match self.vector(index)? {
            Some((start, len)) => (0..len).map(|k| self.fb.i32(start + 4 * k)).collect(),
            None => Ok(Vec::new()),
        }
    }

    fn tables(&self, index: usize) -> Result<Vec<Table<'a>>> {
        match self.vector(index)? {
            Some((start, len)) => (0..len)
                .map(|k| {
                    let slot = start + 4 * k;
                    self.fb.table(self.fb.follow(slot)?)
                })
                .collect(),
            None => Ok(Vec::new()),
        }
    }

    fn string(&self, index: usize) -> Result<Option<String>> {
        match self.vector(index)? {
            Some((start, len)) => Ok(Some(
                String::from_utf8_lossy(self.fb.bytes(start, len)?).into_owned(),
            )),
            None => Ok(None),
        }
    }
}

// ---- the graph ----

/// TensorFlow Lite builtin operator codes (schema.fbs `BuiltinOperator`).
mod code {
    pub const ADD: i32 = 0;
    pub const AVERAGE_POOL_2D: i32 = 1;
    pub const CONV_2D: i32 = 3;
    pub const DEPTHWISE_CONV_2D: i32 = 4;
    pub const DEQUANTIZE: i32 = 6;
    pub const LOGISTIC: i32 = 14;
    pub const MUL: i32 = 18;
    pub const RELU: i32 = 19;
    pub const RESIZE_BILINEAR: i32 = 23;
    pub const CUSTOM: i32 = 32;
    pub const MEAN: i32 = 40;
    pub const HARD_SWISH: i32 = 117;
}

/// TensorFlow Lite tensor types (schema.fbs `TensorType`).
const FLOAT32: u8 = 0;
const FLOAT16: u8 = 1;
const INT32: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activation {
    None,
    Relu,
    Relu6,
}

impl Activation {
    fn from_code(code: u8) -> Result<Self> {
        match code {
            0 => Ok(Self::None),
            1 => Ok(Self::Relu),
            3 => Ok(Self::Relu6),
            other => Err(bad(format!("unsupported fused activation {other}"))),
        }
    }

    fn apply(self, value: f32) -> f32 {
        match self {
            Self::None => value,
            Self::Relu => value.max(0.0),
            Self::Relu6 => value.clamp(0.0, 6.0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Window {
    stride_h: usize,
    stride_w: usize,
    same: bool,
}

#[derive(Clone, Debug)]
enum Kind {
    Conv(Window, Activation),
    Depthwise(Window, Activation),
    Add(Activation),
    Mul(Activation),
    Relu,
    HardSwish,
    Logistic,
    /// Mean over height and width (the only reduction these graphs hold).
    MeanHw,
    AveragePool(Window, usize, usize, Activation),
    ResizeBilinear {
        align_corners: bool,
        half_pixel: bool,
    },
    TransposeBias(Window),
}

#[derive(Clone, Debug)]
struct Op {
    kind: Kind,
    inputs: Vec<usize>,
    output: usize,
}

/// A loaded model: shapes, folded constants and the operators in order.
pub struct Model {
    shapes: Vec<Vec<usize>>,
    constants: Vec<Option<Vec<f32>>>,
    integers: Vec<Option<Vec<i32>>>,
    ops: Vec<Op>,
    input: usize,
    output: usize,
}

impl fmt::Debug for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Model")
            .field("input", &self.input_shape())
            .field("output", &self.shapes[self.output])
            .field("ops", &self.ops.len())
            .finish()
    }
}

fn half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x3ff);
    let value = match (exponent, mantissa) {
        (0, 0) => sign,
        (0, _) => {
            // Subnormal: normalise.
            let mut e = 127 - 15 + 1;
            let mut m = mantissa;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | (e << 23) | ((m & 0x3ff) << 13)
        }
        (0x1f, 0) => sign | 0x7f80_0000,
        (0x1f, _) => sign | 0x7fc0_0000,
        _ => sign | ((exponent + 127 - 15) << 23) | (mantissa << 13),
    };
    f32::from_bits(value)
}

fn window(
    table: Option<Table<'_>>,
    padding: usize,
    stride_w: usize,
    stride_h: usize,
) -> Result<Window> {
    let (same, sw, sh) = match table {
        Some(t) => (
            t.u8_or(padding, 0)? == 0,
            t.i32_or(stride_w, 1)?,
            t.i32_or(stride_h, 1)?,
        ),
        None => (true, 1, 1),
    };
    if sw < 1 || sh < 1 {
        return Err(bad("stride must be positive"));
    }
    Ok(Window {
        stride_h: sh as usize,
        stride_w: sw as usize,
        same,
    })
}

impl Model {
    /// Parses a `.tflite` file. Only the operators listed in the module docs are accepted.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 8 || &bytes[4..8] != b"TFL3" {
            return Err(bad("not a TensorFlow Lite model (no TFL3 identifier)"));
        }
        let fb = Fb(bytes);
        let root = fb.table(fb.follow(0)?)?;
        let codes: Vec<(i32, Option<String>)> = root
            .tables(1)?
            .iter()
            .map(|t| {
                let deprecated = i32::from(t.u8_or(0, 0)? as i8);
                let builtin = t.i32_or(3, 0)?;
                Ok((deprecated.max(builtin), t.string(1)?))
            })
            .collect::<Result<_>>()?;
        let buffers: Vec<&[u8]> = root
            .tables(4)?
            .iter()
            .map(|t| t.bytes(0))
            .collect::<Result<_>>()?;
        let subgraphs = root.tables(2)?;
        let graph = subgraphs.first().ok_or_else(|| bad("model has no graph"))?;
        let tensor_tables = graph.tables(0)?;
        let mut shapes = Vec::with_capacity(tensor_tables.len());
        let mut kinds = Vec::with_capacity(tensor_tables.len());
        let mut constants: Vec<Option<Vec<f32>>> = Vec::with_capacity(tensor_tables.len());
        let mut integers: Vec<Option<Vec<i32>>> = Vec::with_capacity(tensor_tables.len());
        for t in &tensor_tables {
            let shape: Vec<usize> = t
                .ints(0)?
                .into_iter()
                .map(|d| usize::try_from(d).map_err(|_| bad("negative tensor dimension")))
                .collect::<Result<_>>()?;
            let kind = t.u8_or(1, FLOAT32)?;
            let buffer = t.u32_or(2, 0)? as usize;
            if t.table(4)?
                .is_some_and(|q| q.vector(1).ok().flatten().is_some_and(|(_, n)| n > 0))
            {
                return Err(bad("quantised tensors are not supported"));
            }
            let data = buffers.get(buffer).copied().unwrap_or(&[]);
            let count: usize = shape.iter().product();
            let (constant, integer) = match (kind, data.is_empty()) {
                (_, true) => (None, None),
                (FLOAT32, false) if data.len() == count * 4 => (
                    Some(
                        data.chunks_exact(4)
                            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                            .collect(),
                    ),
                    None,
                ),
                (FLOAT16, false) if data.len() == count * 2 => (
                    Some(
                        data.chunks_exact(2)
                            .map(|b| half_to_f32(u16::from_le_bytes([b[0], b[1]])))
                            .collect(),
                    ),
                    None,
                ),
                (INT32, false) if data.len() == count * 4 => (
                    None,
                    Some(
                        data.chunks_exact(4)
                            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                            .collect(),
                    ),
                ),
                _ => return Err(bad("constant tensor has an unsupported type or size")),
            };
            shapes.push(shape);
            kinds.push(kind);
            constants.push(constant);
            integers.push(integer);
        }
        let index = |raw: i32| -> Result<usize> {
            usize::try_from(raw)
                .ok()
                .filter(|&i| i < shapes.len())
                .ok_or_else(|| bad("operator names a tensor that does not exist"))
        };
        let inputs = graph.ints(1)?;
        let outputs = graph.ints(2)?;
        let (&[input], &[output]) = (inputs.as_slice(), outputs.as_slice()) else {
            return Err(bad("model must have one input and one output"));
        };
        let (input, output) = (index(input)?, index(output)?);
        let mut ops = Vec::new();
        for t in graph.tables(3)? {
            let opcode = t.u32_or(0, 0)? as usize;
            let (builtin, custom) = codes
                .get(opcode)
                .ok_or_else(|| bad("operator code out of range"))?;
            let op_inputs: Vec<usize> = t.ints(1)?.into_iter().map(index).collect::<Result<_>>()?;
            let op_outputs = t.ints(2)?;
            let &[out] = op_outputs.as_slice() else {
                return Err(bad("operator must have one output"));
            };
            let out = index(out)?;
            let options = t.table(4)?;
            let act = |field: usize| -> Result<Activation> {
                Activation::from_code(match options {
                    Some(o) => o.u8_or(field, 0)?,
                    None => 0,
                })
            };
            let kind = match *builtin {
                code::DEQUANTIZE => {
                    // Folded: the float16 constant was already widened at load.
                    let source = *op_inputs
                        .first()
                        .ok_or_else(|| bad("DEQUANTIZE without input"))?;
                    let value = constants[source]
                        .clone()
                        .ok_or_else(|| bad("DEQUANTIZE of a non-constant tensor"))?;
                    constants[out] = Some(value);
                    continue;
                }
                code::CONV_2D => Kind::Conv(window(options, 0, 1, 2)?, act(3)?),
                code::DEPTHWISE_CONV_2D => {
                    let multiplier = match options {
                        Some(o) => o.i32_or(3, 1)?,
                        None => 1,
                    };
                    if multiplier != 1 {
                        return Err(bad("depth multiplier other than 1 is not supported"));
                    }
                    Kind::Depthwise(window(options, 0, 1, 2)?, act(4)?)
                }
                code::ADD => Kind::Add(act(0)?),
                code::MUL => Kind::Mul(act(0)?),
                code::RELU => Kind::Relu,
                code::HARD_SWISH => Kind::HardSwish,
                code::LOGISTIC => Kind::Logistic,
                code::MEAN => {
                    let axes = op_inputs
                        .get(1)
                        .and_then(|&a| integers[a].clone())
                        .ok_or_else(|| bad("MEAN without constant axes"))?;
                    let mut sorted = axes;
                    sorted.sort_unstable();
                    if sorted != [1, 2] {
                        return Err(bad("MEAN over axes other than height and width"));
                    }
                    Kind::MeanHw
                }
                code::AVERAGE_POOL_2D => {
                    let o = options.ok_or_else(|| bad("AVERAGE_POOL_2D without options"))?;
                    let fw = o.i32_or(3, 1)?;
                    let fh = o.i32_or(4, 1)?;
                    if fw < 1 || fh < 1 {
                        return Err(bad("pool size must be positive"));
                    }
                    Kind::AveragePool(window(options, 0, 1, 2)?, fh as usize, fw as usize, act(5)?)
                }
                code::RESIZE_BILINEAR => {
                    let (align_corners, half_pixel) = match options {
                        Some(o) => (o.u8_or(2, 0)? != 0, o.u8_or(3, 0)? != 0),
                        None => (false, false),
                    };
                    Kind::ResizeBilinear {
                        align_corners,
                        half_pixel,
                    }
                }
                code::CUSTOM if custom.as_deref() == Some("Convolution2DTransposeBias") => {
                    // MediaPipe's `TfLiteTransposeConvParams`: padding (TfLitePadding: 1 same, 2 valid),
                    // stride_width, stride_height, as three little-endian int32.
                    let raw = t.bytes(5)?;
                    if raw.len() < 12 {
                        return Err(bad("Convolution2DTransposeBias options are too short"));
                    }
                    let read =
                        |k: usize| i32::from_le_bytes([raw[k], raw[k + 1], raw[k + 2], raw[k + 3]]);
                    let (padding, sw, sh) = (read(0), read(4), read(8));
                    if sw < 1 || sh < 1 || !(1..=2).contains(&padding) {
                        return Err(bad("Convolution2DTransposeBias options are invalid"));
                    }
                    Kind::TransposeBias(Window {
                        stride_h: sh as usize,
                        stride_w: sw as usize,
                        same: padding == 1,
                    })
                }
                other => {
                    let name = custom.clone().unwrap_or_else(|| other.to_string());
                    return Err(bad(format!("unsupported operator {name}")));
                }
            };
            ops.push(Op {
                kind,
                inputs: op_inputs,
                output: out,
            });
        }
        let model = Self {
            shapes,
            constants,
            integers,
            ops,
            input,
            output,
        };
        model.check(&kinds)?;
        Ok(model)
    }

    /// Shape checks so that [`Interpreter::run`] cannot index out of range.
    fn check(&self, kinds: &[u8]) -> Result<()> {
        let four = |t: usize| -> Result<[usize; 4]> {
            match self.shapes[t].as_slice() {
                &[n, h, w, c] if n == 1 => Ok([n, h, w, c]),
                _ => Err(bad("expected a 1xHxWxC tensor")),
            }
        };
        let input = four(self.input)?;
        if input[3] != 3 || kinds[self.input] != FLOAT32 {
            return Err(bad("model input must be a float RGB picture"));
        }
        if four(self.output)?[3] != 1 {
            return Err(bad("model output must be a one-channel mask"));
        }
        for op in &self.ops {
            let out = four(op.output)?;
            let first = *op
                .inputs
                .first()
                .ok_or_else(|| bad("operator without input"))?;
            let a = four(first)?;
            let constant = |k: usize| -> Result<&Vec<f32>> {
                op.inputs
                    .get(k)
                    .and_then(|&t| self.constants[t].as_ref())
                    .ok_or_else(|| bad("operator weight is not constant"))
            };
            match &op.kind {
                Kind::Conv(w, _) | Kind::TransposeBias(w) => {
                    let filter = op.inputs.get(1).ok_or_else(|| bad("missing filter"))?;
                    let f = four_any(&self.shapes[*filter])?;
                    if f[3] != a[3] || constant(1)?.len() != f.iter().product::<usize>() {
                        return Err(bad("filter does not match the input channels"));
                    }
                    if out[3] != f[0] || constant(2)?.len() != f[0] {
                        return Err(bad("bias does not match the output channels"));
                    }
                    let expected = match op.kind {
                        Kind::Conv(..) => conv_out(a[1], f[1], w.stride_h, w.same)
                            .zip(conv_out(a[2], f[2], w.stride_w, w.same)),
                        _ => transpose_out(a[1], f[1], w.stride_h, w.same)
                            .zip(transpose_out(a[2], f[2], w.stride_w, w.same)),
                    };
                    if expected != Some((out[1], out[2])) {
                        return Err(bad("convolution output size does not match"));
                    }
                }
                Kind::Depthwise(w, _) => {
                    let f = four_any(&self.shapes[op.inputs[1]])?;
                    if f[3] != a[3]
                        || out[3] != a[3]
                        || constant(1)?.len() != f.iter().product::<usize>()
                        || constant(2)?.len() != a[3]
                    {
                        return Err(bad("depthwise filter does not match"));
                    }
                    if conv_out(a[1], f[1], w.stride_h, w.same)
                        .zip(conv_out(a[2], f[2], w.stride_w, w.same))
                        != Some((out[1], out[2]))
                    {
                        return Err(bad("depthwise output size does not match"));
                    }
                }
                Kind::Add(_) | Kind::Mul(_) => {
                    let second = *op
                        .inputs
                        .get(1)
                        .ok_or_else(|| bad("binary operator needs two inputs"))?;
                    let b = four(second)?;
                    let broadcast =
                        |x: [usize; 4]| x == out || (x[1] == 1 && x[2] == 1 && x[3] == out[3]);
                    if !broadcast(a) || !broadcast(b) {
                        return Err(bad("unsupported broadcast"));
                    }
                }
                Kind::Relu | Kind::HardSwish | Kind::Logistic => {
                    if a != out {
                        return Err(bad("element-wise operator changes shape"));
                    }
                }
                Kind::MeanHw => {
                    if out != [1, 1, 1, a[3]] {
                        return Err(bad("MEAN must keep dimensions"));
                    }
                }
                Kind::AveragePool(w, fh, fw, _) => {
                    if out[3] != a[3]
                        || conv_out(a[1], *fh, w.stride_h, w.same)
                            .zip(conv_out(a[2], *fw, w.stride_w, w.same))
                            != Some((out[1], out[2]))
                    {
                        return Err(bad("pool output size does not match"));
                    }
                }
                Kind::ResizeBilinear { .. } => {
                    if out[3] != a[3] {
                        return Err(bad("resize changes channels"));
                    }
                    if let Some(size) = op.inputs.get(1).and_then(|&t| self.integers[t].as_ref())
                        && size.as_slice() != [out[1] as i32, out[2] as i32]
                    {
                        return Err(bad("resize size does not match its output"));
                    }
                }
            }
        }
        Ok(())
    }

    /// `[height, width]` of the picture the model takes.
    pub fn input_size(&self) -> (usize, usize) {
        let shape = self.input_shape();
        (shape[1], shape[2])
    }

    /// `[height, width]` of the mask it gives.
    pub fn output_size(&self) -> (usize, usize) {
        let shape = &self.shapes[self.output];
        (shape[1], shape[2])
    }

    fn input_shape(&self) -> &[usize] {
        &self.shapes[self.input]
    }
}

fn four_any(shape: &[usize]) -> Result<[usize; 4]> {
    match *shape {
        [a, b, c, d] => Ok([a, b, c, d]),
        _ => Err(bad("expected a four-dimensional filter")),
    }
}

fn conv_out(size: usize, kernel: usize, stride: usize, same: bool) -> Option<usize> {
    if same {
        Some(size.div_ceil(stride))
    } else {
        size.checked_sub(kernel).map(|rest| rest / stride + 1)
    }
}

fn transpose_out(size: usize, kernel: usize, stride: usize, same: bool) -> Option<usize> {
    if same {
        Some(size * stride)
    } else {
        Some((size - 1) * stride + kernel)
    }
}

/// Leading padding of a SAME window (TensorFlow Lite puts the odd pixel at the end).
fn same_pad(size: usize, kernel: usize, stride: usize, out: usize) -> usize {
    (((out - 1) * stride + kernel).saturating_sub(size)) / 2
}

/// The model with its activation buffers, reused from run to run.
pub struct Interpreter {
    model: std::sync::Arc<Model>,
    values: Vec<Vec<f32>>,
}

impl Interpreter {
    pub fn new(model: std::sync::Arc<Model>) -> Self {
        let values = model
            .shapes
            .iter()
            .enumerate()
            .map(|(t, shape)| match &model.constants[t] {
                Some(_) => Vec::new(),
                None => vec![0.0; shape.iter().product()],
            })
            .collect();
        Self { model, values }
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Runs the graph on an RGB picture of the model's input size, values 0 to 1, row after row. Returns the mask.
    pub fn run(&mut self, rgb: &[f32]) -> &[f32] {
        let input = self.model.input;
        let slot = &mut self.values[input];
        let n = slot.len().min(rgb.len());
        slot[..n].copy_from_slice(&rgb[..n]);
        for index in 0..self.model.ops.len() {
            let op = &self.model.ops[index];
            let mut out = std::mem::take(&mut self.values[op.output]);
            let value = |t: usize| -> &[f32] {
                match &self.model.constants[t] {
                    Some(c) => c,
                    None => &self.values[t],
                }
            };
            let shape = |t: usize| -> [usize; 4] {
                let s = &self.model.shapes[t];
                [s[0], s[1], s[2], s[3]]
            };
            let tensor = |t: usize| Tensor {
                data: value(t),
                shape: shape(t),
            };
            let a = op.inputs[0];
            let out_shape = shape(op.output);
            match &op.kind {
                Kind::Conv(w, act) => conv(
                    tensor(a),
                    tensor(op.inputs[1]),
                    value(op.inputs[2]),
                    *w,
                    *act,
                    &mut out,
                    out_shape,
                ),
                Kind::Depthwise(w, act) => depthwise(
                    tensor(a),
                    tensor(op.inputs[1]),
                    value(op.inputs[2]),
                    *w,
                    *act,
                    &mut out,
                    out_shape,
                ),
                Kind::Add(act) => binary(
                    value(a),
                    value(op.inputs[1]),
                    &mut out,
                    out_shape[3],
                    |x, y| act.apply(x + y),
                ),
                Kind::Mul(act) => binary(
                    value(a),
                    value(op.inputs[1]),
                    &mut out,
                    out_shape[3],
                    |x, y| act.apply(x * y),
                ),
                Kind::Relu => unary(value(a), &mut out, |x| x.max(0.0)),
                Kind::HardSwish => {
                    unary(value(a), &mut out, |x| x * (x + 3.0).clamp(0.0, 6.0) / 6.0)
                }
                Kind::Logistic => unary(value(a), &mut out, sigmoid),
                Kind::MeanHw => mean_hw(value(a), shape(a), &mut out),
                Kind::AveragePool(w, fh, fw, act) => {
                    average_pool(tensor(a), *w, *fh, *fw, *act, &mut out, out_shape)
                }
                Kind::ResizeBilinear {
                    align_corners,
                    half_pixel,
                } => resize_bilinear(
                    value(a),
                    shape(a),
                    *align_corners,
                    *half_pixel,
                    &mut out,
                    out_shape,
                ),
                Kind::TransposeBias(w) => transpose_bias(
                    tensor(a),
                    tensor(op.inputs[1]),
                    value(op.inputs[2]),
                    *w,
                    &mut out,
                    out_shape,
                ),
            }
            self.values[op.output] = out;
        }
        &self.values[self.model.output]
    }
}

/// A tensor's values with its NHWC (or OHWI) shape.
#[derive(Clone, Copy)]
struct Tensor<'a> {
    data: &'a [f32],
    shape: [usize; 4],
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

fn unary(a: &[f32], out: &mut [f32], f: impl Fn(f32) -> f32) {
    for (o, &x) in out.iter_mut().zip(a) {
        *o = f(x);
    }
}

/// Same-shape or per-channel broadcast (`[1,1,1,C]`) on either side.
fn binary(a: &[f32], b: &[f32], out: &mut [f32], channels: usize, f: impl Fn(f32, f32) -> f32) {
    let n = out.len();
    let pick = |v: &[f32], i: usize| if v.len() == n { v[i] } else { v[i % channels] };
    for (i, o) in out.iter_mut().enumerate() {
        *o = f(pick(a, i), pick(b, i));
    }
}

fn conv(
    Tensor {
        data: input,
        shape: [_, ih, iw, ic],
    }: Tensor<'_>,
    Tensor {
        data: filter,
        shape: [oc, kh, kw, _],
    }: Tensor<'_>,
    bias: &[f32],
    w: Window,
    act: Activation,
    out: &mut [f32],
    [_, oh, ow, _]: [usize; 4],
) {
    let pad_t = if w.same {
        same_pad(ih, kh, w.stride_h, oh)
    } else {
        0
    };
    let pad_l = if w.same {
        same_pad(iw, kw, w.stride_w, ow)
    } else {
        0
    };
    if kh == 1 && kw == 1 && w.stride_h == 1 && w.stride_w == 1 {
        // Pointwise: one matrix product, rows are pixels.
        for (pixel, row) in input.chunks_exact(ic).zip(out.chunks_exact_mut(oc)) {
            for ((o, weights), b) in row.iter_mut().zip(filter.chunks_exact(ic)).zip(bias) {
                let mut sum = *b;
                for (x, k) in pixel.iter().zip(weights) {
                    sum += x * k;
                }
                *o = act.apply(sum);
            }
        }
        return;
    }
    for oy in 0..oh {
        for ox in 0..ow {
            let row = &mut out[(oy * ow + ox) * oc..(oy * ow + ox + 1) * oc];
            row.copy_from_slice(bias);
            for ky in 0..kh {
                let Some(iy) = (oy * w.stride_h + ky)
                    .checked_sub(pad_t)
                    .filter(|&y| y < ih)
                else {
                    continue;
                };
                for kx in 0..kw {
                    let Some(ix) = (ox * w.stride_w + kx)
                        .checked_sub(pad_l)
                        .filter(|&x| x < iw)
                    else {
                        continue;
                    };
                    let pixel = &input[(iy * iw + ix) * ic..(iy * iw + ix + 1) * ic];
                    for (o, sum) in row.iter_mut().enumerate() {
                        let weights = &filter
                            [((o * kh + ky) * kw + kx) * ic..((o * kh + ky) * kw + kx + 1) * ic];
                        let mut acc = 0.0;
                        for (x, k) in pixel.iter().zip(weights) {
                            acc += x * k;
                        }
                        *sum += acc;
                    }
                }
            }
            for value in row.iter_mut() {
                *value = act.apply(*value);
            }
        }
    }
}

fn depthwise(
    Tensor {
        data: input,
        shape: [_, ih, iw, c],
    }: Tensor<'_>,
    Tensor {
        data: filter,
        shape: [_, kh, kw, _],
    }: Tensor<'_>,
    bias: &[f32],
    w: Window,
    act: Activation,
    out: &mut [f32],
    [_, oh, ow, _]: [usize; 4],
) {
    let pad_t = if w.same {
        same_pad(ih, kh, w.stride_h, oh)
    } else {
        0
    };
    let pad_l = if w.same {
        same_pad(iw, kw, w.stride_w, ow)
    } else {
        0
    };
    for oy in 0..oh {
        for ox in 0..ow {
            let row = &mut out[(oy * ow + ox) * c..(oy * ow + ox + 1) * c];
            row.copy_from_slice(bias);
            for ky in 0..kh {
                let Some(iy) = (oy * w.stride_h + ky)
                    .checked_sub(pad_t)
                    .filter(|&y| y < ih)
                else {
                    continue;
                };
                for kx in 0..kw {
                    let Some(ix) = (ox * w.stride_w + kx)
                        .checked_sub(pad_l)
                        .filter(|&x| x < iw)
                    else {
                        continue;
                    };
                    let pixel = &input[(iy * iw + ix) * c..(iy * iw + ix + 1) * c];
                    let weights = &filter[(ky * kw + kx) * c..(ky * kw + kx + 1) * c];
                    for ((sum, x), k) in row.iter_mut().zip(pixel).zip(weights) {
                        *sum += x * k;
                    }
                }
            }
            for value in row.iter_mut() {
                *value = act.apply(*value);
            }
        }
    }
}

fn mean_hw(input: &[f32], [_, h, w, c]: [usize; 4], out: &mut [f32]) {
    out.fill(0.0);
    for pixel in input.chunks_exact(c) {
        for (o, x) in out.iter_mut().zip(pixel) {
            *o += x;
        }
    }
    let n = (h * w).max(1) as f32;
    for o in out.iter_mut() {
        *o /= n;
    }
}

fn average_pool(
    Tensor {
        data: input,
        shape: [_, ih, iw, c],
    }: Tensor<'_>,
    w: Window,
    fh: usize,
    fw: usize,
    act: Activation,
    out: &mut [f32],
    [_, oh, ow, _]: [usize; 4],
) {
    let pad_t = if w.same {
        same_pad(ih, fh, w.stride_h, oh)
    } else {
        0
    };
    let pad_l = if w.same {
        same_pad(iw, fw, w.stride_w, ow)
    } else {
        0
    };
    for oy in 0..oh {
        for ox in 0..ow {
            let row = &mut out[(oy * ow + ox) * c..(oy * ow + ox + 1) * c];
            row.fill(0.0);
            let mut count = 0usize;
            for ky in 0..fh {
                let Some(iy) = (oy * w.stride_h + ky)
                    .checked_sub(pad_t)
                    .filter(|&y| y < ih)
                else {
                    continue;
                };
                for kx in 0..fw {
                    let Some(ix) = (ox * w.stride_w + kx)
                        .checked_sub(pad_l)
                        .filter(|&x| x < iw)
                    else {
                        continue;
                    };
                    count += 1;
                    for (sum, x) in row
                        .iter_mut()
                        .zip(&input[(iy * iw + ix) * c..(iy * iw + ix + 1) * c])
                    {
                        *sum += x;
                    }
                }
            }
            let n = count.max(1) as f32;
            for value in row.iter_mut() {
                *value = act.apply(*value / n);
            }
        }
    }
}

/// TensorFlow Lite's `ComputeInterpolationValues`: the source position, its lower and upper neighbours.
fn interpolation(value: usize, scale: f32, half_pixel: bool, size: usize) -> (f32, usize, usize) {
    let scaled = if half_pixel {
        (value as f32 + 0.5) * scale - 0.5
    } else {
        value as f32 * scale
    };
    let lower = (scaled.floor().max(0.0) as usize).min(size - 1);
    let upper = (scaled.ceil().max(0.0) as usize).min(size - 1);
    (scaled, lower, upper)
}

fn resize_bilinear(
    input: &[f32],
    [_, ih, iw, c]: [usize; 4],
    align_corners: bool,
    half_pixel: bool,
    out: &mut [f32],
    [_, oh, ow, _]: [usize; 4],
) {
    let scale = |i: usize, o: usize| {
        if align_corners && o > 1 {
            (i - 1) as f32 / (o - 1) as f32
        } else {
            i as f32 / o as f32
        }
    };
    let (sy, sx) = (scale(ih, oh), scale(iw, ow));
    for oy in 0..oh {
        let (y, y0, y1) = interpolation(oy, sy, half_pixel, ih);
        let dy = y - y0 as f32;
        for ox in 0..ow {
            let (x, x0, x1) = interpolation(ox, sx, half_pixel, iw);
            let dx = x - x0 as f32;
            let at = |yy: usize, xx: usize| (yy * iw + xx) * c;
            let row = &mut out[(oy * ow + ox) * c..(oy * ow + ox + 1) * c];
            for (k, o) in row.iter_mut().enumerate() {
                let tl = input[at(y0, x0) + k];
                let tr = input[at(y0, x1) + k];
                let bl = input[at(y1, x0) + k];
                let br = input[at(y1, x1) + k];
                *o = tl * (1.0 - dy) * (1.0 - dx)
                    + tr * (1.0 - dy) * dx
                    + bl * dy * (1.0 - dx)
                    + br * dy * dx;
            }
        }
    }
}

/// MediaPipe's `Convolution2DTransposeBias`: a transposed convolution (OHWI filter) plus a bias.
fn transpose_bias(
    Tensor {
        data: input,
        shape: [_, ih, iw, ic],
    }: Tensor<'_>,
    Tensor {
        data: filter,
        shape: [oc, kh, kw, _],
    }: Tensor<'_>,
    bias: &[f32],
    w: Window,
    out: &mut [f32],
    [_, oh, ow, _]: [usize; 4],
) {
    let pad = |i: usize, k: usize, s: usize, o: usize| {
        if w.same {
            (((i - 1) * s + k).saturating_sub(o)) / 2
        } else {
            0
        }
    };
    let pad_t = pad(ih, kh, w.stride_h, oh);
    let pad_l = pad(iw, kw, w.stride_w, ow);
    for pixel in out.chunks_exact_mut(oc) {
        pixel.copy_from_slice(bias);
    }
    for iy in 0..ih {
        for ix in 0..iw {
            let pixel = &input[(iy * iw + ix) * ic..(iy * iw + ix + 1) * ic];
            for ky in 0..kh {
                let Some(oy) = (iy * w.stride_h + ky)
                    .checked_sub(pad_t)
                    .filter(|&y| y < oh)
                else {
                    continue;
                };
                for kx in 0..kw {
                    let Some(ox) = (ix * w.stride_w + kx)
                        .checked_sub(pad_l)
                        .filter(|&x| x < ow)
                    else {
                        continue;
                    };
                    for o in 0..oc {
                        let weights = &filter
                            [((o * kh + ky) * kw + kx) * ic..((o * kh + ky) * kw + kx + 1) * ic];
                        let mut acc = 0.0;
                        for (x, k) in pixel.iter().zip(weights) {
                            acc += x * k;
                        }
                        out[(oy * ow + ox) * oc + o] += acc;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Instant;

    /// The official build's model files, when the local snapshot has them (it is not part of the repository).
    fn official(name: &str) -> Option<Vec<u8>> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../design/snapshot/desktop/modules/discord_voice-1/discord_voice")
            .join(name);
        std::fs::read(path).ok()
    }

    #[test]
    fn half_floats_widen_exactly() {
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0xc000), -2.0);
        assert_eq!(half_to_f32(0x3555), 0.333_251_95);
        assert_eq!(half_to_f32(0x0001), 5.960_464_5e-8);
        assert_eq!(half_to_f32(0x7c00), f32::INFINITY);
        assert_eq!(half_to_f32(0x8000).to_bits(), (-0.0f32).to_bits());
    }

    #[test]
    fn junk_is_refused_without_panicking() {
        assert!(Model::parse(b"").is_err());
        assert!(Model::parse(b"\x10\0\0\0TFL3").is_err());
        let mut noise = vec![0u8; 4096];
        let mut state = 7u32;
        for byte in &mut noise {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *byte = (state >> 16) as u8;
        }
        noise[4..8].copy_from_slice(b"TFL3");
        assert!(Model::parse(&noise).is_err());
    }

    #[test]
    fn same_padding_puts_the_odd_pixel_last() {
        assert_eq!(conv_out(4, 3, 2, true), Some(2));
        assert_eq!(same_pad(4, 3, 2, 2), 0);
        assert_eq!(same_pad(3, 3, 2, 2), 1);
        assert_eq!(same_pad(5, 3, 2, 3), 1);
        assert_eq!(conv_out(16, 16, 16, false), Some(1));
    }

    #[test]
    fn a_strided_conv_matches_a_hand_sum() {
        // 1x3x3x1 input 1..9, one 3x3 filter of ones, stride 2 SAME: 2x2 outputs, one padding pixel before each axis.
        let input: Vec<f32> = (1..=9).map(|v| v as f32).collect();
        let filter = vec![1.0; 9];
        let mut out = vec![0.0; 4];
        conv(
            Tensor {
                data: &input,
                shape: [1, 3, 3, 1],
            },
            Tensor {
                data: &filter,
                shape: [1, 3, 3, 1],
            },
            &[0.5],
            Window {
                stride_h: 2,
                stride_w: 2,
                same: true,
            },
            Activation::None,
            &mut out,
            [1, 2, 2, 1],
        );
        assert_eq!(out, vec![12.5, 16.5, 24.5, 28.5]);
    }

    #[test]
    fn transpose_bias_spreads_each_pixel_over_its_block() {
        let input = vec![1.0, 2.0];
        let filter = vec![1.0, 10.0, 100.0, 1000.0];
        let mut out = vec![0.0; 8];
        transpose_bias(
            Tensor {
                data: &input,
                shape: [1, 1, 2, 1],
            },
            Tensor {
                data: &filter,
                shape: [1, 2, 2, 1],
            },
            &[0.25],
            Window {
                stride_h: 2,
                stride_w: 2,
                same: true,
            },
            &mut out,
            [1, 2, 4, 1],
        );
        assert_eq!(
            out,
            vec![1.25, 10.25, 2.25, 20.25, 100.25, 1000.25, 200.25, 2000.25]
        );
    }

    #[test]
    fn half_pixel_resize_keeps_edges_and_interpolates_inside() {
        let input = vec![0.0, 4.0];
        let mut out = vec![0.0; 4];
        resize_bilinear(&input, [1, 1, 2, 1], false, true, &mut out, [1, 1, 4, 1]);
        assert_eq!(out, vec![0.0, 1.0, 3.0, 4.0]);
    }

    #[test]
    fn the_official_models_load_and_run() {
        for (name, input) in [
            ("selfie_segmentation_landscape.tflite", (144, 256)),
            ("selfie_segmentation.tflite", (256, 256)),
        ] {
            let Some(bytes) = official(name) else {
                eprintln!("{name} is not in the local snapshot; skipped");
                continue;
            };
            let model = Model::parse(&bytes).expect(name);
            assert_eq!(model.input_size(), input, "{name}");
            assert_eq!(model.output_size(), input, "{name}");
            let mut interpreter = Interpreter::new(Arc::new(model));
            let (h, w) = input;
            // A grey room with a skin-coloured disc in the middle.
            let mut rgb = vec![0.0f32; h * w * 3];
            for y in 0..h {
                for x in 0..w {
                    let (dx, dy) = (x as f32 - w as f32 / 2.0, y as f32 - h as f32 * 0.55);
                    let inside = dx * dx + dy * dy < (h as f32 * 0.3).powi(2);
                    let colour = if inside {
                        [0.85, 0.65, 0.55]
                    } else {
                        [0.35, 0.37, 0.4]
                    };
                    rgb[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&colour);
                }
            }
            let started = Instant::now();
            let mask = interpreter.run(&rgb).to_vec();
            let first = started.elapsed();
            let started = Instant::now();
            let again = interpreter.run(&rgb).to_vec();
            eprintln!(
                "{name}: first run {first:?}, second {:?}",
                started.elapsed()
            );
            assert_eq!(mask.len(), h * w);
            assert!(
                mask.iter().all(|v| (0.0..=1.0).contains(v)),
                "{name} gives probabilities"
            );
            assert_eq!(mask, again, "{name} is deterministic");
        }
    }
}
