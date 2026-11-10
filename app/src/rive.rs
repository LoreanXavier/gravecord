//! The first frame of a Rive file (`.riv`). The Shop draws its Nitro credit coins from one: the official client
//! (module 883888) plays the file's `STACKED COINS` and `SINGLE COIN` artboards with the Rive canvas runtime and, under
//! reduced motion, halts on the first frame. Here the file is read directly (the binary layout and the core type and
//! property keys are the published Rive runtime's, MIT licensed), the artboard is posed the way its first runtime frame
//! is, and tiny-skia fills the shapes. Only what the Shop's art uses is implemented: nodes, shapes, points paths,
//! rectangles and ellipses, solid and linear gradient fills, strokes, clipping shapes, translation and distance
//! constraints, the entry animation at time zero and the joystick's two animations.

use image::ImageEncoder;
use std::collections::HashMap;
use tiny_skia::{
    BlendMode, Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, Path,
    PathBuilder, Pixmap, Point, Shader, SpreadMode, Stroke, Transform,
};

const MAX_FILE: usize = 2 * 1024 * 1024;
const MAX_OBJECTS: usize = 200_000;
const MAX_SIZE: u32 = 1024;

// Core type keys.
const ARTBOARD: u16 = 1;
const NODE: u16 = 2;
const SHAPE: u16 = 3;
const ELLIPSE: u16 = 4;
const STRAIGHT_VERTEX: u16 = 5;
const CUBIC_DETACHED_VERTEX: u16 = 6;
const RECTANGLE: u16 = 7;
const POINTS_PATH: u16 = 16;
const SOLID_COLOR: u16 = 18;
const GRADIENT_STOP: u16 = 19;
const FILL: u16 = 20;
const LINEAR_GRADIENT: u16 = 22;
const STROKE: u16 = 24;
const KEYED_OBJECT: u16 = 25;
const KEYED_PROPERTY: u16 = 26;
const CUBIC_EASE: u16 = 28;
const KEY_FRAME_DOUBLE: u16 = 30;
const LINEAR_ANIMATION: u16 = 31;
const CUBIC_ASYMMETRIC_VERTEX: u16 = 34;
const CUBIC_MIRRORED_VERTEX: u16 = 35;
const CLIPPING_SHAPE: u16 = 42;
const STATE_MACHINE: u16 = 53;
const STATE_MACHINE_LAYER: u16 = 57;
const ANIMATION_STATE: u16 = 61;
const ANY_STATE: u16 = 62;
const ENTRY_STATE: u16 = 63;
const EXIT_STATE: u16 = 64;
const STATE_TRANSITION: u16 = 65;
const DISTANCE_CONSTRAINT: u16 = 82;
const TRANSLATION_CONSTRAINT: u16 = 87;
const JOYSTICK: u16 = 148;

// Property keys.
const NAME: u16 = 4;
const PARENT_ID: u16 = 5;
const ARTBOARD_WIDTH: u16 = 7;
const ARTBOARD_HEIGHT: u16 = 8;
const X: u16 = 13;
const Y: u16 = 14;
const ROTATION: u16 = 15;
const SCALE_X: u16 = 16;
const SCALE_Y: u16 = 17;
const OPACITY: u16 = 18;
const WIDTH: u16 = 20;
const HEIGHT: u16 = 21;
const BLEND_MODE: u16 = 23;
const VERTEX_X: u16 = 24;
const VERTEX_Y: u16 = 25;
const CORNER_RADIUS: u16 = 31;
const IS_CLOSED: u16 = 32;
const GRADIENT_START_Y: u16 = 33;
const GRADIENT_END_X: u16 = 34;
const GRADIENT_END_Y: u16 = 35;
const SOLID_COLOR_VALUE: u16 = 37;
const STOP_COLOR: u16 = 38;
const STOP_POSITION: u16 = 39;
const FILL_RULE: u16 = 40;
const IS_VISIBLE: u16 = 41;
const GRADIENT_START_X: u16 = 42;
const GRADIENT_OPACITY: u16 = 46;
const THICKNESS: u16 = 47;
const STROKE_CAP: u16 = 48;
const STROKE_JOIN: u16 = 49;
const OBJECT_ID: u16 = 51;
const PROPERTY_KEY: u16 = 53;
const FPS: u16 = 56;
const DURATION: u16 = 57;
const FRAME: u16 = 67;
const INTERPOLATION: u16 = 68;
const INTERPOLATOR_ID: u16 = 69;
const KEY_VALUE: u16 = 70;
const ROTATION_ASYMMETRIC: u16 = 79;
const IN_DISTANCE_ASYMMETRIC: u16 = 80;
const OUT_DISTANCE_ASYMMETRIC: u16 = 81;
const ROTATION_MIRRORED: u16 = 82;
const DISTANCE_MIRRORED: u16 = 83;
const IN_ROTATION: u16 = 84;
const IN_DISTANCE: u16 = 85;
const OUT_ROTATION: u16 = 86;
const OUT_DISTANCE: u16 = 87;
const SOURCE_ID: u16 = 92;
const CLIP_FILL_RULE: u16 = 93;
const CLIP_VISIBLE: u16 = 94;
const ORIGIN_X: u16 = 123;
const ORIGIN_Y: u16 = 124;
const DRAWABLE_FLAGS: u16 = 129;
const ANIMATION_ID: u16 = 149;
const STATE_TO_ID: u16 = 151;
const STRENGTH: u16 = 172;
const TARGET_ID: u16 = 173;
const DISTANCE: u16 = 177;
const DISTANCE_MODE: u16 = 178;
const COPY_FACTOR: u16 = 182;
const OFFSET: u16 = 188;
const DOES_COPY: u16 = 189;
const COPY_FACTOR_Y: u16 = 185;
const DOES_COPY_Y: u16 = 192;
const DEFAULT_STATE_MACHINE: u16 = 236;
const JOYSTICK_POS_X: u16 = 303;
const JOYSTICK_POS_Y: u16 = 304;
const JOYSTICK_X_ANIMATION: u16 = 301;
const JOYSTICK_Y_ANIMATION: u16 = 302;
const JOYSTICK_WIDTH: u16 = 305;
const JOYSTICK_HEIGHT: u16 = 306;
const JOYSTICK_ORIGIN_X: u16 = 307;
const JOYSTICK_ORIGIN_Y: u16 = 308;
const JOYSTICK_HANDLE: u16 = 313;
/// The cubic interpolators come with two key sets: the standalone one and the component one.
const EASE_KEYS: [[u16; 4]; 2] = [[63, 64, 65, 66], [337, 338, 339, 340]];
/// The control point distance of a quarter circle drawn with one cubic (`circleConstant` of the runtime).
const CIRCLE: f32 = 0.552_284_8;

/// The type of a property the file's table of contents does not name: the core properties, as the runtime's generated
/// headers declare them (0 unsigned, 1 string, 2 float, 3 color).
const CORE_KINDS: &[(u16, u8)] = &[
    (4, 1),
    (5, 0),
    (7, 2),
    (8, 2),
    (13, 2),
    (14, 2),
    (15, 2),
    (16, 2),
    (17, 2),
    (18, 2),
    (20, 2),
    (21, 2),
    (23, 0),
    (24, 2),
    (25, 2),
    (31, 2),
    (32, 0),
    (33, 2),
    (34, 2),
    (35, 2),
    (37, 3),
    (38, 3),
    (39, 2),
    (40, 0),
    (42, 2),
    (46, 2),
    (47, 2),
    (51, 0),
    (53, 0),
    (55, 1),
    (59, 0),
    (63, 2),
    (64, 2),
    (65, 2),
    (66, 2),
    (67, 0),
    (68, 0),
    (69, 0),
    (70, 2),
    (79, 2),
    (80, 2),
    (81, 2),
    (82, 2),
    (83, 2),
    (84, 2),
    (85, 2),
    (86, 2),
    (87, 2),
    (92, 0),
    (128, 0),
    (129, 0),
    (138, 1),
    (149, 0),
    (151, 0),
    (155, 0),
    (156, 0),
    (158, 0),
];

#[derive(Clone, Debug)]
enum Value {
    Uint(u64),
    Float(f32),
    Color(u32),
    Text(Vec<u8>),
}

struct Object {
    ty: u16,
    props: Vec<(u16, Value)>,
}

impl Object {
    fn raw(&self, key: u16) -> Option<&Value> {
        self.props
            .iter()
            .find_map(|(k, v)| (*k == key).then_some(v))
    }

    fn float(&self, key: u16, default: f32) -> f32 {
        match self.raw(key) {
            Some(Value::Float(v)) => *v,
            Some(Value::Uint(v)) => *v as f32,
            _ => default,
        }
    }

    fn uint(&self, key: u16, default: u64) -> u64 {
        match self.raw(key) {
            Some(Value::Uint(v)) => *v,
            Some(Value::Float(v)) => *v as u64,
            _ => default,
        }
    }

    fn text(&self, key: u16) -> &[u8] {
        match self.raw(key) {
            Some(Value::Text(text)) => text,
            _ => &[],
        }
    }

    fn color(&self, key: u16) -> u32 {
        match self.raw(key) {
            Some(Value::Color(v)) => *v,
            _ => 0xFF75_7575,
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(byte)
    }

    fn varuint(&mut self) -> Option<u64> {
        let (mut value, mut shift) = (0u64, 0u32);
        loop {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f).checked_shl(shift)?;
            if byte & 0x80 == 0 {
                return Some(value);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }

    fn take(&mut self, len: usize) -> Option<&[u8]> {
        let slice = self.data.get(self.pos..self.pos.checked_add(len)?)?;
        self.pos += len;
        Some(slice)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}

/// The objects of a file in stream order. The header holds the version, the file id, the keys of the properties
/// whose type the file states (two bits each, four to a byte) and then the objects, each a type key and its
/// `key, value` pairs ended by a zero key.
fn parse(data: &[u8]) -> Option<Vec<Object>> {
    if data.len() > MAX_FILE || data.get(..4)? != b"RIVE" {
        return None;
    }
    let mut reader = Reader { data, pos: 4 };
    for _ in 0..3 {
        reader.varuint()?;
    }
    let mut keys = Vec::new();
    loop {
        let key = reader.varuint()?;
        if key == 0 {
            break;
        }
        keys.push(u16::try_from(key).ok()?);
    }
    let mut kinds: HashMap<u16, u8> = CORE_KINDS.iter().copied().collect();
    for chunk in keys.chunks(4) {
        let bits = reader.u32()?;
        for (slot, key) in chunk.iter().enumerate() {
            kinds.insert(*key, ((bits >> (slot * 2)) & 3) as u8);
        }
    }
    let mut objects = Vec::new();
    while reader.pos < data.len() {
        let ty = u16::try_from(reader.varuint()?).ok()?;
        let mut props = Vec::new();
        loop {
            let key = u16::try_from(reader.varuint()?).ok()?;
            if key == 0 {
                break;
            }
            let value = match *kinds.get(&key)? {
                0 => Value::Uint(reader.varuint()?),
                1 => {
                    let len = usize::try_from(reader.varuint()?).ok()?;
                    Value::Text(reader.take(len)?.to_vec())
                }
                2 => Value::Float(f32::from_le_bytes(reader.take(4)?.try_into().ok()?)),
                _ => Value::Color(reader.u32()?),
            };
            props.push((key, value));
        }
        objects.push(Object { ty, props });
        if objects.len() > MAX_OBJECTS {
            return None;
        }
    }
    Some(objects)
}

/// `[a b c d e f]`: `x' = a x + c y + e`, `y' = b x + d y + f`, the runtime's `Mat2D` layout.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine([f32; 6]);

impl Affine {
    #[cfg(test)]
    const IDENTITY: Affine = Affine([1., 0., 0., 1., 0., 0.]);

    fn compose(x: f32, y: f32, rotation: f32, sx: f32, sy: f32) -> Self {
        let (sin, cos) = rotation.sin_cos();
        Affine([cos * sx, sin * sx, -sin * sy, cos * sy, x, y])
    }

    /// `self * other`: `other` first, then `self`.
    fn then(&self, other: &Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [oa, ob, oc, od, oe, of] = other.0;
        Affine([
            a * oa + c * ob,
            b * oa + d * ob,
            a * oc + c * od,
            b * oc + d * od,
            a * oe + c * of + e,
            b * oe + d * of + f,
        ])
    }

    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    fn invert(&self) -> Option<Affine> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1. / det;
        let (ia, ib, ic, id) = (d * inv, -b * inv, -c * inv, a * inv);
        Some(Affine([
            ia,
            ib,
            ic,
            id,
            -(ia * e + ic * f),
            -(ib * e + id * f),
        ]))
    }

    fn scale(&self) -> f32 {
        (self.0[0] * self.0[3] - self.0[1] * self.0[2]).abs().sqrt()
    }
}

struct Key {
    frame: f32,
    value: f32,
    interpolation: u64,
    ease: [f32; 4],
}

struct Keyed {
    object: usize,
    properties: Vec<(u16, Vec<Key>)>,
}

struct Animation {
    fps: f32,
    duration: f32,
    keyed: Vec<Keyed>,
}

impl Animation {
    fn seconds(&self) -> f32 {
        self.duration / self.fps
    }
}

/// The value between two keys at `frame`: hold, linear, or eased by the first key's cubic interpolator.
fn sample(keys: &[Key], frame: f32) -> Option<f32> {
    let first = keys.first()?;
    let last = keys.last()?;
    if frame <= first.frame {
        return Some(first.value);
    }
    if frame >= last.frame {
        return Some(last.value);
    }
    let at = keys.iter().rposition(|key| key.frame <= frame)?;
    let (from, to) = (&keys[at], &keys[at + 1]);
    let factor = (frame - from.frame) / (to.frame - from.frame);
    let eased = match from.interpolation {
        0 => return Some(from.value),
        2 => ease(from.ease, factor),
        _ => factor,
    };
    Some(from.value + (to.value - from.value) * eased)
}

/// A CSS-style cubic bezier easing curve through (0,0) and (1,1): the y at the t where x equals `factor`.
fn ease([x1, y1, x2, y2]: [f32; 4], factor: f32) -> f32 {
    let bezier = |a: f32, b: f32, t: f32| {
        let inverse = 1. - t;
        3. * inverse * inverse * t * a + 3. * inverse * t * t * b + t * t * t
    };
    let (mut low, mut high) = (0f32, 1f32);
    for _ in 0..40 {
        let mid = (low + high) / 2.;
        if bezier(x1, x2, mid) < factor {
            low = mid;
        } else {
            high = mid;
        }
    }
    bezier(y1, y2, (low + high) / 2.)
}

/// One artboard: its components (index 0 is the artboard itself) with their tree, and its animations.
struct Board<'a> {
    comps: Vec<&'a Object>,
    parent: Vec<Option<usize>>,
    children: Vec<Vec<usize>>,
    animations: Vec<Animation>,
    entry: Option<usize>,
    /// Property values the animations and the joystick set over the file's.
    over: HashMap<(usize, u16), f32>,
}

fn board<'a>(objects: &'a [Object], name: &str) -> Option<Board<'a>> {
    let start = objects
        .iter()
        .position(|object| object.ty == ARTBOARD && object.text(NAME) == name.as_bytes())?;
    let end = objects[start + 1..]
        .iter()
        .position(|object| object.ty == ARTBOARD)
        .map_or(objects.len(), |at| start + 1 + at);
    let section = &objects[start..end];
    let split = section
        .iter()
        .position(|object| object.ty == LINEAR_ANIMATION || object.ty == STATE_MACHINE)
        .unwrap_or(section.len());
    let comps: Vec<&Object> = section[..split].iter().collect();
    let mut parent = vec![None; comps.len()];
    let mut children = vec![Vec::new(); comps.len()];
    for (index, comp) in comps.iter().enumerate().skip(1) {
        let at = usize::try_from(comp.uint(PARENT_ID, 0)).ok()?;
        if at < comps.len() {
            parent[index] = Some(at);
            children[at].push(index);
        }
    }
    let animations = animations(&section[split..], &comps);
    let entry = entry_animation(&section[split..], comps[0], animations.len());
    Some(Board {
        comps,
        parent,
        children,
        animations,
        entry,
        over: HashMap::new(),
    })
}

fn animations(section: &[Object], comps: &[&Object]) -> Vec<Animation> {
    let mut out: Vec<Animation> = Vec::new();
    for object in section {
        match object.ty {
            LINEAR_ANIMATION => out.push(Animation {
                fps: object.float(FPS, 60.).max(1.),
                duration: object.float(DURATION, 60.),
                keyed: Vec::new(),
            }),
            STATE_MACHINE => break,
            KEYED_OBJECT => {
                if let Some(animation) = out.last_mut() {
                    animation.keyed.push(Keyed {
                        object: usize::try_from(object.uint(OBJECT_ID, 0)).unwrap_or(usize::MAX),
                        properties: Vec::new(),
                    });
                }
            }
            KEYED_PROPERTY => {
                if let Some(keyed) = out.last_mut().and_then(|a| a.keyed.last_mut()) {
                    keyed.properties.push((
                        u16::try_from(object.uint(PROPERTY_KEY, 0)).unwrap_or(0),
                        vec![],
                    ));
                }
            }
            KEY_FRAME_DOUBLE => {
                let interpolator = usize::try_from(object.uint(INTERPOLATOR_ID, u64::MAX))
                    .ok()
                    .and_then(|id| comps.get(id));
                let ease = interpolator.filter(|comp| comp.ty == CUBIC_EASE).map_or(
                    [0.42, 0., 0.58, 1.],
                    |comp| {
                        let keys = if comp.props.iter().any(|(k, _)| EASE_KEYS[1].contains(k)) {
                            EASE_KEYS[1]
                        } else {
                            EASE_KEYS[0]
                        };
                        [
                            comp.float(keys[0], 0.42),
                            comp.float(keys[1], 0.),
                            comp.float(keys[2], 0.58),
                            comp.float(keys[3], 1.),
                        ]
                    },
                );
                if let Some(list) = out
                    .last_mut()
                    .and_then(|a| a.keyed.last_mut())
                    .and_then(|k| k.properties.last_mut())
                {
                    list.1.push(Key {
                        frame: object.float(FRAME, 0.),
                        value: object.float(KEY_VALUE, 0.),
                        interpolation: object.uint(INTERPOLATION, 0),
                        ease,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// The animation the default state machine's entry state goes to: the first transition of the entry state names a
/// state of the first layer, and an animation state names the animation.
fn entry_animation(section: &[Object], artboard: &Object, count: usize) -> Option<usize> {
    let machine = usize::try_from(artboard.uint(DEFAULT_STATE_MACHINE, 0)).ok()?;
    let start = section
        .iter()
        .enumerate()
        .filter(|(_, object)| object.ty == STATE_MACHINE)
        .nth(machine)?
        .0;
    let layer = section[start..]
        .iter()
        .position(|object| object.ty == STATE_MACHINE_LAYER)?
        + start;
    let end = section[layer + 1..]
        .iter()
        .position(|object| matches!(object.ty, STATE_MACHINE | STATE_MACHINE_LAYER))
        .map_or(section.len(), |at| layer + 1 + at);
    let states: Vec<usize> = (layer + 1..end)
        .filter(|at| {
            matches!(
                section[*at].ty,
                ANIMATION_STATE | ANY_STATE | ENTRY_STATE | EXIT_STATE
            )
        })
        .collect();
    let entry = *states.iter().find(|at| section[**at].ty == ENTRY_STATE)?;
    let transition = section[entry + 1..end]
        .iter()
        .take_while(|object| {
            !matches!(
                object.ty,
                ANIMATION_STATE | ANY_STATE | ENTRY_STATE | EXIT_STATE
            )
        })
        .find(|object| object.ty == STATE_TRANSITION)?;
    let target = *states.get(usize::try_from(transition.uint(STATE_TO_ID, u64::MAX)).ok()?)?;
    let animation = usize::try_from(section[target].uint(ANIMATION_ID, u64::MAX)).ok()?;
    (section[target].ty == ANIMATION_STATE && animation < count).then_some(animation)
}

impl Board<'_> {
    fn num(&self, index: usize, key: u16, default: f32) -> f32 {
        self.over
            .get(&(index, key))
            .copied()
            .unwrap_or_else(|| self.comps[index].float(key, default))
    }

    fn apply(&mut self, animation: usize, seconds: f32) {
        let Some(animation) = self.animations.get(animation) else {
            return;
        };
        let frame = seconds * animation.fps;
        let mut set = Vec::new();
        for keyed in &animation.keyed {
            for (key, keys) in &keyed.properties {
                if let Some(value) = sample(keys, frame) {
                    set.push(((keyed.object, *key), value));
                }
            }
        }
        self.over.extend(set);
    }

    fn is_transform(&self, index: usize) -> bool {
        matches!(
            self.comps[index].ty,
            NODE | SHAPE | ELLIPSE | RECTANGLE | POINTS_PATH
        )
    }

    fn local(&self, index: usize) -> Affine {
        Affine::compose(
            self.num(index, X, 0.),
            self.num(index, Y, 0.),
            self.num(index, ROTATION, 0.),
            self.num(index, SCALE_X, 1.),
            self.num(index, SCALE_Y, 1.),
        )
    }

    /// The world transform of a component: its parent's, its own local, then its constraints in file order.
    fn world(
        &self,
        index: usize,
        root: &Affine,
        cache: &mut HashMap<usize, Affine>,
        depth: u32,
    ) -> Affine {
        if let Some(world) = cache.get(&index) {
            return *world;
        }
        if depth > 64 {
            return *root;
        }
        let base = match self.parent[index] {
            Some(parent) if self.is_transform(parent) => self.world(parent, root, cache, depth + 1),
            _ => *root,
        };
        let mut world = base.then(&self.local(index));
        for child in &self.children[index] {
            match self.comps[*child].ty {
                TRANSLATION_CONSTRAINT => {
                    self.translate(index, *child, &mut world, root, cache, depth)
                }
                DISTANCE_CONSTRAINT => self.distance(*child, &mut world, root, cache, depth),
                _ => {}
            }
        }
        cache.insert(index, world);
        world
    }

    fn target_world(
        &self,
        constraint: usize,
        root: &Affine,
        cache: &mut HashMap<usize, Affine>,
        depth: u32,
    ) -> Option<Affine> {
        let target = usize::try_from(self.comps[constraint].uint(TARGET_ID, u64::MAX)).ok()?;
        (target < self.comps.len() && self.is_transform(target))
            .then(|| self.world(target, root, cache, depth + 1))
    }

    /// `TranslationConstraint::constrain` for world-space source and destination: the component's world translation
    /// moves to the target's (scaled by the copy factor), plus its own local translation when `offset` is set.
    fn translate(
        &self,
        index: usize,
        constraint: usize,
        world: &mut Affine,
        root: &Affine,
        cache: &mut HashMap<usize, Affine>,
        depth: u32,
    ) {
        let Some(target) = self.target_world(constraint, root, cache, depth) else {
            return;
        };
        let data = self.comps[constraint];
        let strength = self.num(constraint, STRENGTH, 1.);
        let offset = data.uint(OFFSET, 0) != 0;
        let (own_x, own_y) = (world.0[4], world.0[5]);
        let mut wanted = (own_x, own_y);
        if data.uint(DOES_COPY, 1) != 0 {
            wanted.0 = target.0[4] * data.float(COPY_FACTOR, 1.)
                + if offset { self.num(index, X, 0.) } else { 0. };
        }
        if data.uint(DOES_COPY_Y, 1) != 0 {
            wanted.1 = target.0[5] * data.float(COPY_FACTOR_Y, 1.)
                + if offset { self.num(index, Y, 0.) } else { 0. };
        }
        world.0[4] = own_x * (1. - strength) + wanted.0 * strength;
        world.0[5] = own_y * (1. - strength) + wanted.1 * strength;
    }

    /// `DistanceConstraint::constrain`: keeps the component at `distance` from the target (the default mode only
    /// pulls it in when it is further away).
    fn distance(
        &self,
        constraint: usize,
        world: &mut Affine,
        root: &Affine,
        cache: &mut HashMap<usize, Affine>,
        depth: u32,
    ) {
        let Some(target) = self.target_world(constraint, root, cache, depth) else {
            return;
        };
        let data = self.comps[constraint];
        let (tx, ty) = (target.0[4], target.0[5]);
        let (ox, oy) = (world.0[4], world.0[5]);
        let (dx, dy) = (ox - tx, oy - ty);
        let current = dx.hypot(dy);
        let wanted = self.num(constraint, DISTANCE, 100.);
        match data.uint(DISTANCE_MODE, 0) {
            0 if current < wanted => return,
            1 if current > wanted => return,
            _ => {}
        }
        if current < 0.001 {
            return;
        }
        let strength = self.num(constraint, STRENGTH, 1.);
        let (px, py) = (tx + dx * wanted / current, ty + dy * wanted / current);
        world.0[4] = ox + (px - ox) * strength;
        world.0[5] = oy + (py - oy) * strength;
    }

    /// The world opacity: the product of every transform ancestor's opacity and the component's own.
    fn opacity(&self, index: usize) -> f32 {
        let mut value = 1.;
        let mut at = Some(index);
        while let Some(current) = at {
            if self.is_transform(current) {
                value *= self.num(current, OPACITY, 1.);
            }
            at = self.parent[current];
        }
        value
    }

    /// Poses the artboard the way the first frame of the runtime does: the entry animation at time zero, then the
    /// joystick, which maps the handle's place inside it to the middle of its two animations and back to the handle
    /// again (a few rounds settle it).
    fn pose(&mut self, root: &Affine) {
        let joystick = self.comps.iter().position(|comp| comp.ty == JOYSTICK);
        let (mut jx, mut jy) = (0f32, 0f32);
        for round in 0..4 {
            self.over.clear();
            if let Some(entry) = self.entry {
                self.apply(entry, 0.);
            }
            let Some(joystick) = joystick else { return };
            let data = self.comps[joystick];
            for (key, value) in [(JOYSTICK_X_ANIMATION, jx), (JOYSTICK_Y_ANIMATION, jy)] {
                if let Ok(animation) = usize::try_from(data.uint(key, u64::MAX))
                    && let Some(seconds) = self.animations.get(animation).map(Animation::seconds)
                {
                    self.apply(animation, (value + 1.) / 2. * seconds);
                }
            }
            if round == 3 {
                return;
            }
            let mut cache = HashMap::new();
            let handle = usize::try_from(data.uint(JOYSTICK_HANDLE, u64::MAX)).ok();
            let (Some(handle), Some(parent)) = (handle, self.parent[joystick]) else {
                return;
            };
            if handle >= self.comps.len() {
                return;
            }
            let parent_world = self.world(parent, root, &mut cache, 0);
            let handle_world = self.world(handle, root, &mut cache, 0);
            let placed = parent_world.then(&Affine::compose(
                data.float(JOYSTICK_POS_X, 0.),
                data.float(JOYSTICK_POS_Y, 0.),
                0.,
                1.,
                1.,
            ));
            let Some(inverse) = placed.invert() else {
                return;
            };
            let (px, py) = inverse.apply(handle_world.0[4], handle_world.0[5]);
            let (w, h) = (
                data.float(JOYSTICK_WIDTH, 100.),
                data.float(JOYSTICK_HEIGHT, 100.),
            );
            let left = -w * data.float(JOYSTICK_ORIGIN_X, 0.5);
            let top = -h * data.float(JOYSTICK_ORIGIN_Y, 0.5);
            jx = if w == 0. {
                0.
            } else {
                (px - left) * 2. / w - 1.
            };
            jy = if h == 0. {
                0.
            } else {
                (py - top) * 2. / h - 1.
            };
        }
    }

    /// The world-space outline of every path under a shape, in one path.
    fn outline(
        &self,
        shape: usize,
        root: &Affine,
        cache: &mut HashMap<usize, Affine>,
    ) -> Option<Path> {
        let mut builder = PathBuilder::new();
        for path in &self.children[shape] {
            let world = self.world(*path, root, cache, 0);
            match self.comps[*path].ty {
                POINTS_PATH => self.points_path(*path, &world, &mut builder),
                RECTANGLE => self.rectangle(*path, &world, &mut builder),
                ELLIPSE => self.ellipse(*path, &world, &mut builder),
                _ => {}
            }
        }
        builder.finish()
    }

    fn points_path(&self, path: usize, world: &Affine, builder: &mut PathBuilder) {
        // Each vertex as its point, its in handle and its out handle.
        let vertices: Vec<[(f32, f32); 3]> = self.children[path]
            .iter()
            .filter_map(|vertex| self.vertex(*vertex))
            .collect();
        if vertices.len() < 2 {
            return;
        }
        let at = |(x, y): (f32, f32)| world.apply(x, y);
        let first = vertices[0];
        let (sx, sy) = at(first[0]);
        builder.move_to(sx, sy);
        let mut out = first[2];
        for vertex in &vertices[1..] {
            let (c1, c2, end) = (at(out), at(vertex[1]), at(vertex[0]));
            builder.cubic_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
            out = vertex[2];
        }
        if self.comps[path].uint(IS_CLOSED, 0) != 0 {
            let (c1, c2, end) = (at(out), at(first[1]), at(first[0]));
            builder.cubic_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
            builder.close();
        }
    }

    /// A vertex as `[point, in, out]`: straight vertices have both handles on the point.
    fn vertex(&self, index: usize) -> Option<[(f32, f32); 3]> {
        let point = (self.num(index, VERTEX_X, 0.), self.num(index, VERTEX_Y, 0.));
        let handle = |rotation: f32, distance: f32, sign: f32| {
            (
                point.0 + sign * rotation.cos() * distance,
                point.1 + sign * rotation.sin() * distance,
            )
        };
        Some(match self.comps[index].ty {
            STRAIGHT_VERTEX => [point, point, point],
            CUBIC_DETACHED_VERTEX => [
                point,
                handle(
                    self.num(index, IN_ROTATION, 0.),
                    self.num(index, IN_DISTANCE, 0.),
                    1.,
                ),
                handle(
                    self.num(index, OUT_ROTATION, 0.),
                    self.num(index, OUT_DISTANCE, 0.),
                    1.,
                ),
            ],
            CUBIC_ASYMMETRIC_VERTEX => {
                let rotation = self.num(index, ROTATION_ASYMMETRIC, 0.);
                [
                    point,
                    handle(rotation, self.num(index, IN_DISTANCE_ASYMMETRIC, 0.), -1.),
                    handle(rotation, self.num(index, OUT_DISTANCE_ASYMMETRIC, 0.), 1.),
                ]
            }
            CUBIC_MIRRORED_VERTEX => {
                let rotation = self.num(index, ROTATION_MIRRORED, 0.);
                let distance = self.num(index, DISTANCE_MIRRORED, 0.);
                [
                    point,
                    handle(rotation, distance, -1.),
                    handle(rotation, distance, 1.),
                ]
            }
            _ => return None,
        })
    }

    fn rectangle(&self, path: usize, world: &Affine, builder: &mut PathBuilder) {
        let width = self.num(path, WIDTH, 0.);
        let height = self.num(path, HEIGHT, 0.);
        let left = -self.num(path, ORIGIN_X, 0.5) * width;
        let top = -self.num(path, ORIGIN_Y, 0.5) * height;
        let radius = self
            .num(path, CORNER_RADIUS, 0.)
            .min(width / 2.)
            .min(height / 2.)
            .max(0.);
        let at = |x: f32, y: f32| world.apply(x, y);
        let (right, bottom) = (left + width, top + height);
        let line = |builder: &mut PathBuilder, x: f32, y: f32, first: bool| {
            let (x, y) = at(x, y);
            if first {
                builder.move_to(x, y);
            } else {
                builder.line_to(x, y);
            }
        };
        if radius <= 0. {
            line(builder, left, top, true);
            line(builder, right, top, false);
            line(builder, right, bottom, false);
            line(builder, left, bottom, false);
            builder.close();
            return;
        }
        let k = radius * CIRCLE;
        line(builder, left + radius, top, true);
        line(builder, right - radius, top, false);
        let curve = |builder: &mut PathBuilder, c1: (f32, f32), c2: (f32, f32), e: (f32, f32)| {
            let (c1, c2, e) = (at(c1.0, c1.1), at(c2.0, c2.1), at(e.0, e.1));
            builder.cubic_to(c1.0, c1.1, c2.0, c2.1, e.0, e.1);
        };
        curve(
            builder,
            (right - radius + k, top),
            (right, top + radius - k),
            (right, top + radius),
        );
        line(builder, right, bottom - radius, false);
        curve(
            builder,
            (right, bottom - radius + k),
            (right - radius + k, bottom),
            (right - radius, bottom),
        );
        line(builder, left + radius, bottom, false);
        curve(
            builder,
            (left + radius - k, bottom),
            (left, bottom - radius + k),
            (left, bottom - radius),
        );
        line(builder, left, top + radius, false);
        curve(
            builder,
            (left, top + radius - k),
            (left + radius - k, top),
            (left + radius, top),
        );
        builder.close();
    }

    fn ellipse(&self, path: usize, world: &Affine, builder: &mut PathBuilder) {
        let rx = self.num(path, WIDTH, 0.) / 2.;
        let ry = self.num(path, HEIGHT, 0.) / 2.;
        let ox = -self.num(path, ORIGIN_X, 0.5) * rx * 2. + rx;
        let oy = -self.num(path, ORIGIN_Y, 0.5) * ry * 2. + ry;
        let at = |x: f32, y: f32| world.apply(x, y);
        let start = at(ox, oy - ry);
        builder.move_to(start.0, start.1);
        let mut cubic = |c1: (f32, f32), c2: (f32, f32), e: (f32, f32)| {
            let (c1, c2, e) = (at(c1.0, c1.1), at(c2.0, c2.1), at(e.0, e.1));
            builder.cubic_to(c1.0, c1.1, c2.0, c2.1, e.0, e.1);
        };
        cubic(
            (ox + rx * CIRCLE, oy - ry),
            (ox + rx, oy - ry * CIRCLE),
            (ox + rx, oy),
        );
        cubic(
            (ox + rx, oy + ry * CIRCLE),
            (ox + rx * CIRCLE, oy + ry),
            (ox, oy + ry),
        );
        cubic(
            (ox - rx * CIRCLE, oy + ry),
            (ox - rx, oy + ry * CIRCLE),
            (ox - rx, oy),
        );
        cubic(
            (ox - rx, oy - ry * CIRCLE),
            (ox - rx * CIRCLE, oy - ry),
            (ox, oy - ry),
        );
        builder.close();
    }

    /// Whether a drawable is hidden by its flags.
    fn hidden(&self, index: usize) -> bool {
        self.comps[index].uint(DRAWABLE_FLAGS, 0) & 1 != 0
    }

    /// The clipping shapes whose parent is the drawable or one of its ancestors.
    fn clips(&self, index: usize) -> Vec<usize> {
        let mut found = Vec::new();
        let mut at = Some(index);
        while let Some(current) = at {
            for child in &self.children[current] {
                if self.comps[*child].ty == CLIPPING_SHAPE
                    && self.comps[*child].uint(CLIP_VISIBLE, 1) != 0
                {
                    found.push(*child);
                }
            }
            at = self.parent[current];
        }
        found
    }

    /// The coverage of a clipping shape: every shape under its source node, filled together.
    fn clip_mask(
        &self,
        clip: usize,
        root: &Affine,
        cache: &mut HashMap<usize, Affine>,
        size: u32,
    ) -> Option<Mask> {
        let source = usize::try_from(self.comps[clip].uint(SOURCE_ID, u64::MAX)).ok()?;
        if source >= self.comps.len() {
            return None;
        }
        let mut builder = PathBuilder::new();
        let mut stack = vec![source];
        while let Some(current) = stack.pop() {
            if self.comps[current].ty == SHAPE
                && let Some(path) = self.outline(current, root, cache)
            {
                builder.push_path(&path);
            }
            stack.extend(self.children[current].iter().copied());
        }
        let path = builder.finish()?;
        let mut mask = Mask::new(size, size)?;
        let rule = if self.comps[clip].uint(CLIP_FILL_RULE, 0) == 1 {
            FillRule::EvenOdd
        } else {
            FillRule::Winding
        };
        mask.fill_path(&path, rule, true, Transform::identity());
        Some(mask)
    }

    fn paint(&self, mutator: usize, shape_world: &Affine, opacity: f32) -> Option<Paint<'static>> {
        let data = self.comps[mutator];
        let shader = match data.ty {
            SOLID_COLOR => Shader::SolidColor(color(data.color(SOLID_COLOR_VALUE), opacity)),
            LINEAR_GRADIENT => {
                let mut stops: Vec<(f32, u32)> = self.children[mutator]
                    .iter()
                    .filter(|stop| self.comps[**stop].ty == GRADIENT_STOP)
                    .map(|stop| {
                        let stop = self.comps[*stop];
                        (stop.float(STOP_POSITION, 0.), stop.color(STOP_COLOR))
                    })
                    .collect();
                stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                let opacity = opacity * self.num(mutator, GRADIENT_OPACITY, 1.);
                let start = shape_world.apply(
                    self.num(mutator, GRADIENT_START_X, 0.),
                    self.num(mutator, GRADIENT_START_Y, 0.),
                );
                let end = shape_world.apply(
                    self.num(mutator, GRADIENT_END_X, 0.),
                    self.num(mutator, GRADIENT_END_Y, 0.),
                );
                let colors: Vec<(f32, Color)> = stops
                    .iter()
                    .map(|(at, value)| (at.clamp(0., 1.), color(*value, opacity)))
                    .collect();
                let first = colors.first()?.1;
                if colors.len() == 1 {
                    Shader::SolidColor(first)
                } else {
                    LinearGradient::new(
                        Point::from_xy(start.0, start.1),
                        Point::from_xy(end.0, end.1),
                        colors
                            .iter()
                            .map(|(at, value)| GradientStop::new(*at, *value))
                            .collect(),
                        SpreadMode::Pad,
                        Transform::identity(),
                    )
                    .unwrap_or(Shader::SolidColor(first))
                }
            }
            _ => return None,
        };
        Some(Paint {
            shader,
            anti_alias: true,
            ..Paint::default()
        })
    }
}

fn color(value: u32, opacity: f32) -> Color {
    let [a, r, g, b] = value.to_be_bytes();
    Color::from_rgba8(
        r,
        g,
        b,
        (f32::from(a) * opacity.clamp(0., 1.)).round() as u8,
    )
}

fn blend_mode(value: u64) -> BlendMode {
    match value {
        12 => BlendMode::Plus,
        14 => BlendMode::Screen,
        15 => BlendMode::Overlay,
        16 => BlendMode::Darken,
        17 => BlendMode::Lighten,
        18 => BlendMode::ColorDodge,
        19 => BlendMode::ColorBurn,
        20 => BlendMode::HardLight,
        21 => BlendMode::SoftLight,
        22 => BlendMode::Difference,
        23 => BlendMode::Exclusion,
        24 => BlendMode::Multiply,
        25 => BlendMode::Hue,
        26 => BlendMode::Saturation,
        27 => BlendMode::Color,
        28 => BlendMode::Luminosity,
        _ => BlendMode::SourceOver,
    }
}

/// The artboard `name` (`STACKED COINS` or `SINGLE COIN`) at its first frame, `size` pixels square, as PNG bytes.
pub fn first_frame_png(riv: &[u8], name: &str, size: u32) -> Option<Vec<u8>> {
    let objects = parse(riv)?;
    let mut board = board(&objects, name)?;
    let size = size.clamp(8, MAX_SIZE);
    let width = board.comps[0].float(ARTBOARD_WIDTH, 0.);
    let height = board.comps[0].float(ARTBOARD_HEIGHT, 0.);
    if width <= 0. || height <= 0. {
        return None;
    }
    let scale = size as f32 / width.max(height);
    let root = Affine([scale, 0., 0., scale, 0., 0.]);
    board.pose(&root);
    let mut pixmap = Pixmap::new(size, size)?;
    let mut cache = HashMap::new();
    let mut masks: HashMap<usize, Option<Mask>> = HashMap::new();
    // The runtime draws the last drawable of the file first.
    for shape in (1..board.comps.len()).rev() {
        if board.comps[shape].ty != SHAPE || board.hidden(shape) {
            continue;
        }
        let opacity = board.opacity(shape);
        if opacity <= 0. {
            continue;
        }
        // A rectangle that is only stroked is an alignment frame of the art; it is not part of the picture.
        let guide = board.children[shape]
            .iter()
            .all(|child| board.comps[*child].ty != FILL)
            && board.children[shape]
                .iter()
                .any(|child| board.comps[*child].ty == RECTANGLE);
        if guide {
            continue;
        }
        let Some(path) = board.outline(shape, &root, &mut cache) else {
            continue;
        };
        let shape_world = board.world(shape, &root, &mut cache, 0);
        let mode = blend_mode(board.comps[shape].uint(BLEND_MODE, 3));
        let mut mask: Option<Mask> = None;
        for clip in board.clips(shape) {
            let entry = masks
                .entry(clip)
                .or_insert_with(|| board.clip_mask(clip, &root, &mut cache, size));
            let Some(entry) = entry else { continue };
            match mask.as_mut() {
                None => mask = Some(entry.clone()),
                Some(current) => {
                    for (kept, other) in current.data_mut().iter_mut().zip(entry.data()) {
                        *kept = ((u16::from(*kept) * u16::from(*other) + 127) / 255) as u8;
                    }
                }
            }
        }
        for paint in &board.children[shape] {
            let data = board.comps[*paint];
            if !matches!(data.ty, FILL | STROKE) || data.uint(IS_VISIBLE, 1) == 0 {
                continue;
            }
            let Some(mutator) = board.children[*paint]
                .iter()
                .copied()
                .find(|child| matches!(board.comps[*child].ty, SOLID_COLOR | LINEAR_GRADIENT))
            else {
                continue;
            };
            let Some(mut drawn) = board.paint(mutator, &shape_world, opacity) else {
                continue;
            };
            drawn.blend_mode = mode;
            if data.ty == FILL {
                let rule = if data.uint(FILL_RULE, 0) == 1 {
                    FillRule::EvenOdd
                } else {
                    FillRule::Winding
                };
                pixmap.fill_path(&path, &drawn, rule, Transform::identity(), mask.as_ref());
            } else {
                let stroke = Stroke {
                    width: board.num(*paint, THICKNESS, 1.) * shape_world.scale(),
                    line_cap: match data.uint(STROKE_CAP, 0) {
                        1 => LineCap::Round,
                        2 => LineCap::Square,
                        _ => LineCap::Butt,
                    },
                    line_join: match data.uint(STROKE_JOIN, 0) {
                        1 => LineJoin::Round,
                        2 => LineJoin::Bevel,
                        _ => LineJoin::Miter,
                    },
                    ..Stroke::default()
                };
                pixmap.stroke_path(&path, &drawn, &stroke, Transform::identity(), mask.as_ref());
            }
        }
    }
    // A hairline stroke runs along the artboard edge in the file; the artboard clip hides it in the runtime.
    let last = size as usize - 1;
    for edge in 0..size as usize {
        for (x, y) in [(edge, 0), (edge, last), (0, edge), (last, edge)] {
            let at = (y * size as usize + x) * 4;
            pixmap.data_mut()[at..at + 4].fill(0);
        }
    }
    let mut pixels = pixmap.data().to_vec();
    // The pixmap holds premultiplied pixels; PNG holds straight alpha.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&pixels, size, size, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file with one 100 by 100 artboard holding a red rectangle shape: the objects `Artboard`, `Shape`,
    /// `Rectangle`, `Fill`, `SolidColor`.
    fn tiny() -> Vec<u8> {
        let mut file = b"RIVE".to_vec();
        file.extend([7, 0, 0]);
        // No keys with stated types: the core table applies.
        file.push(0);
        let float = |key: u8, value: f32| {
            let mut out = vec![key];
            out.extend(value.to_le_bytes());
            out
        };
        let mut push = |ty: u8, props: Vec<Vec<u8>>| {
            file.push(ty);
            for prop in props {
                file.extend(prop);
            }
            file.push(0);
        };
        push(
            1,
            vec![
                vec![
                    4, 13, b'S', b'T', b'A', b'C', b'K', b'E', b'D', b' ', b'C', b'O', b'I', b'N',
                    b'S',
                ],
                float(7, 100.),
                float(8, 100.),
            ],
        );
        push(3, vec![vec![5, 0], float(13, 50.), float(14, 50.)]);
        push(7, vec![vec![5, 1], float(20, 40.), float(21, 40.)]);
        push(20, vec![vec![5, 1]]);
        let mut color = vec![37];
        color.extend(0xFFFF_0000u32.to_le_bytes());
        push(18, vec![vec![5, 3], color]);
        file
    }

    #[test]
    fn a_shape_with_a_solid_fill_is_drawn_where_it_sits() {
        let objects = parse(&tiny()).expect("parses");
        assert_eq!(objects.len(), 5);
        let mut board = board(&objects, "STACKED COINS").expect("artboard");
        assert_eq!(board.comps.len(), 5);
        let root = Affine::IDENTITY;
        board.pose(&root);
        let mut cache = HashMap::new();
        let path = board.outline(1, &root, &mut cache).expect("outline");
        let bounds = path.bounds();
        assert_eq!(
            (bounds.left(), bounds.top(), bounds.right(), bounds.bottom()),
            (30., 30., 70., 70.)
        );
    }

    #[test]
    fn a_file_that_is_not_rive_is_refused() {
        assert!(first_frame_png(b"not a rive file", "STACKED COINS", 64).is_none());
    }

    #[test]
    fn matrices_compose_and_invert() {
        let a = Affine::compose(10., 20., 0.5, 2., 3.);
        let b = a.invert().expect("invertible");
        let (x, y) = b.apply(a.apply(4., 5.).0, a.apply(4., 5.).1);
        assert!((x - 4.).abs() < 1e-4 && (y - 5.).abs() < 1e-4);
    }

    #[test]
    fn easing_runs_from_zero_to_one() {
        let curve = [0.42, 0., 0.58, 1.];
        assert!(ease(curve, 0.).abs() < 1e-3);
        assert!((ease(curve, 1.) - 1.).abs() < 1e-3);
        assert!((ease(curve, 0.5) - 0.5).abs() < 1e-2);
    }
}
