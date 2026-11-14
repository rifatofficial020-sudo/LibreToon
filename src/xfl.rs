//! Best-effort Animate/XFL import. Scripts are data to skip, never executable.
//! Geometry errors are isolated to their element; container/XML errors remain fatal.
use crate::{
    animation::{self, Curve, Tween},
    document::{Document, Layer, Mark},
    features::{self, Gradient, Instance, Playback, Stop, Symbol, SymbolClock},
    rig::{self, IDENTITY, Matrix},
    shared::SharedVec,
};
use eframe::egui::{Pos2, Rect, Vec2};
use std::collections::{BTreeSet, HashMap};
use xml::{
    attribute::OwnedAttribute,
    reader::{ParserConfig, XmlEvent},
};
#[derive(Default)]
struct Node {
    name: String,
    attrs: Vec<OwnedAttribute>,
    children: Vec<Node>,
    text: String,
}
impl Node {
    fn a(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name.local_name == key)
            .map(|a| a.value.as_str())
    }
    fn n(&self, key: &str, default: f32) -> Result<f32, String> {
        crate::fla::number(&self.attrs, key, default)
    }
    fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|n| n.name == name)
    }
    fn path(&self, path: &str) -> Option<&Self> {
        path.split('/').try_fold(self, |n, p| n.child(p))
    }
    fn descendants<'a>(&'a self, name: &str, out: &mut Vec<&'a Self>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.descendants(name, out);
        }
    }
    fn matrix(&self) -> Result<Matrix, String> {
        let Some(m) = self.path("matrix/Matrix") else {
            return Ok(IDENTITY);
        };
        Ok([
            m.n("a", 1.)?,
            m.n("b", 0.)?,
            m.n("c", 0.)?,
            m.n("d", 1.)?,
            m.n("tx", 0.)?,
            m.n("ty", 0.)?,
        ])
    }
}
fn parse(xml: &str) -> Result<Node, String> {
    let mut stack = vec![Node::default()];
    let mut count = 0usize;
    let mut text = 0usize;
    let reader = ParserConfig::new()
        .max_entity_expansion_depth(8)
        .max_entity_expansion_length(65536)
        .max_name_length(1024)
        .max_attributes(256)
        .max_attribute_length(8 * 1024 * 1024)
        .max_data_length(64 * 1024 * 1024)
        .create_reader(xml.as_bytes());
    for event in reader {
        match event.map_err(|e| format!("XML XFL no válido: {e}"))? {
            XmlEvent::StartElement {
                name, attributes, ..
            } => {
                count += 1;
                if stack.len() > 96 || count > 500_000 {
                    return Err("XML XFL excede los límites de complejidad".into());
                }
                stack.push(Node {
                    name: name.local_name,
                    attrs: attributes,
                    ..Default::default()
                });
            }
            XmlEvent::EndElement { .. } => {
                let node = stack.pop().ok_or("XML incompleto")?;
                stack.last_mut().ok_or("XML sin raíz")?.children.push(node);
            }
            XmlEvent::Characters(s) | XmlEvent::CData(s) => {
                text += s.len();
                if text > 64 * 1024 * 1024 {
                    return Err("Texto XFL demasiado grande".into());
                }
                stack.last_mut().unwrap().text.push_str(&s);
            }
            _ => {}
        }
    }
    let mut root = stack.pop().unwrap();
    if root.children.len() != 1 {
        return Err("XML XFL sin raíz única".into());
    }
    Ok(root.children.remove(0))
}
struct Importer<'a> {
    archive: crate::fla_zip::Archive<'a>,
    names: HashMap<String, u64>,
    bitmaps: HashMap<String, (u64, u32, u32)>,
    kinds: HashMap<u64, String>,
    warnings: BTreeSet<String>,
    next_track: u64,
    context: String,
    total: usize,
}
impl Importer<'_> {
    fn warn(&mut self, s: impl Into<String>) {
        if self.warnings.len() < 200 {
            let mut s = s.into();
            if s.len() > 2048 {
                let mut end = 2048;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                s.truncate(end);
                s.push('…');
            }
            if !self.context.is_empty() {
                s = format!("{}: {s}", self.context);
            }
            self.warnings.insert(s);
        }
    }
    fn account(&mut self, count: usize) -> Result<(), String> {
        self.total = self.total.saturating_add(count);
        if self.total > 500_000 {
            Err("FLA excede el presupuesto de geometría importada".into())
        } else {
            Ok(())
        }
    }
    fn element(&mut self, n: &Node, born: usize) -> Result<Vec<Mark>, String> {
        let mut marks = match n.name.as_str() {
            "DOMShape" => self.shape(n)?,
            "DOMGroup" => {
                let mut out = vec![];
                if let Some(members) = n.child("members") {
                    for c in &members.children {
                        out.extend(self.best_element(c, born));
                    }
                }
                out
            }
            "DOMBitmapInstance" => {
                let name = n.a("libraryItemName").unwrap_or("");
                let Some(&(id, width, height)) = self.bitmaps.get(name) else {
                    self.warn(format!(
                        "DOMBitmapInstance sin imagen incrustada compatible: {name}"
                    ));
                    return Ok(vec![]);
                };
                let mut mark = Mark {
                    points: vec![[0., 0.], [width as f32, height as f32]].into(),
                    rectangle: true,
                    closed: true,
                    stroke: false,
                    fill: Some([0, 0, 0, 0]),
                    ..Default::default()
                };
                mark.style.media_image = Some(id);
                vec![mark]
            }
            "DOMSymbolInstance" => {
                let name = n.a("libraryItemName").unwrap_or("");
                let Some(id) = self.names.get(name).copied() else {
                    self.warn(format!("Símbolo no disponible: {name}"));
                    return Ok(vec![]);
                };
                let kind = n
                    .a("symbolType")
                    .map(str::to_owned)
                    .or_else(|| self.kinds.get(&id).cloned())
                    .unwrap_or_else(|| "movie clip".into());
                let button = kind == "button";
                let mode = if button {
                    self.warn(
                        "Botones: se conserva el estado visual Up; se omiten interacción y código",
                    );
                    Playback::Single
                } else {
                    match n.a("loop").unwrap_or("loop") {
                        "single frame" => Playback::Single,
                        "play once" => Playback::Once,
                        _ => Playback::Loop,
                    }
                };
                let mut m = Mark {
                    points: vec![[0., 0.], [1., 1.]].into(),
                    rectangle: true,
                    closed: true,
                    stroke: false,
                    fill: Some([0, 0, 0, 0]),
                    ..Default::default()
                };
                m.style.instance = Some(Instance {
                    symbol: id,
                    matrix: IDENTITY,
                    mode,
                    first: integer(n, "firstFrame", 0, 9999)?,
                    born,
                    clock: if kind == "movie clip" {
                        SymbolClock::Independent
                    } else {
                        SymbolClock::Scene
                    },
                });
                vec![m]
            }
            "DOMStaticText" | "DOMDynamicText" | "DOMInputText" | "DOMTLFText" => self.text(n)?,
            "DOMRectangleObject" | "DOMOvalObject" => {
                let x = n.n("x", 0.)?;
                let y = n.n("y", 0.)?;
                let w = n.n("objectWidth", n.n("width", 1.)?)?;
                let h = n.n("objectHeight", n.n("height", 1.)?)?;
                let mut base = Mark {
                    rectangle: true,
                    closed: true,
                    stroke: false,
                    fill: Some([0, 0, 0, 255]),
                    points: vec![[x, y], [x + w, y + h]].into(),
                    ..Default::default()
                };
                if let Some(fill) = n.path("fill/SolidColor") {
                    base.fill = Some(crate::fla::parse_color(&fill.attrs)?);
                }
                if n.name == "DOMOvalObject" {
                    base.rectangle = false;
                    base.points = (0..96)
                        .map(|i| {
                            let a = i as f32 / 96. * std::f32::consts::TAU;
                            [x + w / 2. + w / 2. * a.cos(), y + h / 2. + h / 2. * a.sin()]
                        })
                        .collect();
                }
                self.warn(
                    "Primitivas de Animate: aproximación geométrica sin parámetros de esquina/arco",
                );
                vec![base]
            }
            "ActionScript" | "script" | "DOMFrameSound" => {
                self.warn("ActionScript y JavaScript se omiten; no se ejecuta código");
                vec![]
            }
            _ => {
                self.warn(format!("Elemento omitido: {}", n.name));
                vec![]
            }
        };
        let matrix = n.matrix()?;
        for m in &mut marks {
            *m = features::affine(m, matrix);
        }
        if let Some(c) = n.path("color/Color") {
            for m in &mut marks {
                if m.style.instance.is_some() {
                    let mut values = [0.; 8];
                    for (i, key) in ["red", "green", "blue", "alpha"].iter().enumerate() {
                        values[i] = c.n(&format!("{key}Multiplier"), 1.)?;
                        values[i + 4] = c.n(&format!("{key}Offset"), 0.)?;
                    }
                    m.style.instance_color = Some(values);
                }
                let convert = |color: &mut [u8; 4]| -> Result<(), String> {
                    for (i, key) in ["red", "green", "blue", "alpha"].iter().enumerate() {
                        color[i] = (color[i] as f32 * c.n(&format!("{key}Multiplier"), 1.)?
                            + c.n(&format!("{key}Offset"), 0.)?)
                        .round()
                        .clamp(0., 255.) as u8;
                    }
                    Ok(())
                };
                convert(&mut m.color)?;
                if let Some(fill) = m.fill.as_mut() {
                    convert(fill)?;
                }
                if let Some(g) = m.style.gradient.as_mut() {
                    for stop in &mut g.stops {
                        convert(&mut stop.color)?;
                    }
                }
            }
            if c.a("tintColor").is_some() || c.a("brightness").is_some() {
                self.warn(
                    "Tinte/brillo de instancia aproximado; se conservan multiplicadores RGBA",
                );
            }
        }
        if n.child("filters").is_some()
            || n.a("blendMode")
                .is_some_and(|s| s != "normal" && s != "layer")
        {
            self.warn("Filtros y modos de fusión especiales omitidos");
        }
        self.account(
            marks
                .iter()
                .map(|m| {
                    1 + m.points.len()
                        + m.nodes.len() * 3
                        + m.holes.iter().map(|h| h.len()).sum::<usize>()
                })
                .sum(),
        )?;
        Ok(marks)
    }
    fn best_element(&mut self, n: &Node, born: usize) -> Vec<Mark> {
        match self.element(n, born) {
            Ok(m) => m,
            Err(e) => {
                self.warn(format!("{} omitido: {e}", n.name));
                vec![]
            }
        }
    }
    fn fill(&mut self, n: &Node) -> Result<([u8; 4], Option<Gradient>), String> {
        if let Some(s) = n.child("SolidColor") {
            return Ok((crate::fla::parse_color(&s.attrs)?, None));
        }
        let g = n
            .child("LinearGradient")
            .or_else(|| n.child("RadialGradient"));
        if let Some(g) = g {
            let radial = g.name == "RadialGradient";
            let matrix = g.matrix()?;
            let mut stops = vec![];
            for s in &g.children {
                if s.name == "GradientEntry" {
                    stops.push(Stop {
                        t: s.n("ratio", 0.)?.clamp(0., 1.),
                        color: crate::fla::parse_color(&s.attrs)?,
                    });
                }
            }
            stops.sort_by(|a, b| a.t.total_cmp(&b.t));
            if stops.len() < 2 {
                return Err("Degradado sin dos colores".into());
            }
            if stops.len() > 32 {
                self.warn("Degradado reducido a 32 colores");
                stops.truncate(32);
            }
            let start = rig::point(matrix, Pos2::new(if radial { 0. } else { -819.2 }, 0.));
            let end = rig::point(matrix, Pos2::new(819.2, 0.));
            return Ok((
                stops[0].color,
                Some(Gradient {
                    radial,
                    start: [start.x, start.y],
                    end: [end.x, end.y],
                    stops: stops.into(),
                }),
            ));
        }
        Err("Relleno de bitmap u otro estilo no compatible".into())
    }
    fn shape(&mut self, n: &Node) -> Result<Vec<Mark>, String> {
        let mut fills = HashMap::new();
        if let Some(list) = n.child("fills") {
            for f in &list.children {
                match self.fill(f) {
                    Ok(style) => {
                        fills.insert(f.a("index").unwrap_or("1").to_owned(), style);
                    }
                    Err(e) => self.warn(e),
                }
            }
        }
        let mut strokes = HashMap::new();
        if let Some(list) = n.child("strokes") {
            for s in &list.children {
                if let Some(style) = s.children.first() {
                    if style.name != "SolidStroke" {
                        self.warn("Contornos de textura aproximados por contorno sólido");
                    }
                    let fill = style
                        .child("fill")
                        .and_then(|f| self.fill(f).ok())
                        .unwrap_or(([0, 0, 0, 255], None));
                    strokes.insert(
                        s.a("index").unwrap_or("1").to_owned(),
                        (style.n("weight", 1.)?.clamp(0.01, 100.), fill.0),
                    );
                }
            }
        }
        let mut edges: HashMap<String, Vec<Vec<[f32; 2]>>> = HashMap::new();
        let mut out = vec![];
        if let Some(list) = n.child("edges") {
            for e in &list.children {
                if e.name != "Edge" {
                    continue;
                }
                if e.a("cubics").is_some() {
                    self.warn(
                        "Metadatos cúbicos auxiliares XFL omitidos; se usa la geometría edges",
                    );
                }
                let paths = crate::fla::editable_paths(e.a("edges").unwrap_or(""))?;
                for mut m in paths {
                    let points: Vec<_> = crate::vector::flatten(&m, 0.04)
                        .into_iter()
                        .map(crate::vector::xy)
                        .collect();
                    self.account(points.len())?;
                    let stroke = e.a("strokeStyle").unwrap_or("0");
                    if let Some((width, color)) = strokes.get(stroke) {
                        m.width = *width;
                        m.color = *color;
                        out.push(m);
                    }
                    let left = e.a("fillStyle0").unwrap_or("0");
                    let right = e.a("fillStyle1").unwrap_or("0");
                    if left != right {
                        if right != "0" {
                            edges.entry(right.into()).or_default().push(points.clone());
                        }
                        if left != "0" {
                            let mut p = points;
                            p.reverse();
                            edges.entry(left.into()).or_default().push(p);
                        }
                    }
                }
            }
        }
        let mut keys: Vec<_> = edges.keys().cloned().collect();
        keys.sort();
        for key in keys {
            let Some((color, gradient)) = fills.get(&key) else {
                self.warn("Relleno omitido: estilo no disponible");
                continue;
            };
            match crate::fla::join_fill_rings(edges.remove(&key).unwrap(), *color) {
                Ok(mut m) => {
                    for m in &mut m {
                        m.style.gradient = gradient.clone();
                    }
                    m.append(&mut out);
                    out = m;
                }
                Err(e) => self.warn(format!("Relleno omitido: {e}")),
            }
        }
        Ok(out)
    }
    fn text(&mut self, n: &Node) -> Result<Vec<Mark>, String> {
        let mut runs = vec![];
        n.descendants("DOMTextRun", &mut runs);
        if runs.is_empty() {
            self.warn("Texto TLF no compatible omitido");
            return Ok(vec![]);
        }
        let fonts = crate::typography::system_fonts();
        let mut out = vec![];
        let mut x = n.n("left", 0.)?;
        let y = n.n("top", 0.)?;
        for run in runs {
            let Some(chars) = run.child("characters") else {
                continue;
            };
            let a = run.path("textAttrs/DOMTextAttrs");
            let size = a.map_or(Ok(12.), |a| a.n("size", 12.))?.clamp(1., 2048.);
            let face = a.and_then(|a| a.a("face")).unwrap_or("");
            let wanted = face.replace([' ', '-'], "").to_lowercase();
            let font = fonts
                .iter()
                .find(|(name, _)| name.replace([' ', '-'], "").to_lowercase() == wanted)
                .or(fonts.first());
            let Some((name, path)) = font else {
                self.warn("Texto omitido: no hay fuentes locales disponibles");
                continue;
            };
            if !name.eq_ignore_ascii_case(face) {
                self.warn(format!("Fuente sustituida: {face} → {name}"));
            }
            let mut spec = crate::typography::TextSpec {
                content: chars.text.clone(),
                font: path.to_string_lossy().into_owned(),
                size,
                matrix: [1., 0., 0., 1., x, y + size],
                ..Default::default()
            };
            if let Some(a) = a {
                if let Some(hex) = a.a("fillColor") {
                    spec.color = color_hex(hex)?;
                }
                spec.letter_spacing = a.n("letterSpacing", 0.)?;
            }
            let m = crate::typography::geometry(&spec)?;
            x = crate::vector::control_bounds(&m).right();
            out.push(m);
        }
        self.warn("Texto importado como contornos; tipografía y disposición pueden variar");
        Ok(out)
    }
    fn timeline(
        &mut self,
        n: &Node,
    ) -> Result<(SharedVec<Layer>, SharedVec<features::Folder>), String> {
        let mut layers = vec![];
        let Some(list) = n.child("layers") else {
            return Err("Línea de tiempo sin capas".into());
        };
        let parent_index = |index: usize| {
            list.children[index]
                .a("parentLayerIndex")
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|&parent| parent < index)
        };
        let mut folder_ids = HashMap::new();
        let mut folders = Vec::new();
        for (index, node) in list.children.iter().enumerate() {
            if node.name != "DOMLayer" || node.a("layerType") != Some("folder") {
                continue;
            }
            if folders.len() >= 200 {
                self.warn("Carpetas adicionales omitidas: se alcanzó el límite de 200");
                break;
            }
            let id = features::id();
            folders.push(features::Folder {
                id,
                name: node.a("name").unwrap_or("Folder").into(),
                parent: parent_index(index).and_then(|parent| folder_ids.get(&parent).copied()),
                collapsed: node.a("isExpanded") == Some("false"),
                transform: Default::default(),
                timeline_slot: None,
                range: None,
                source_shift: 0,
            });
            folder_ids.insert(index, id);
        }
        let mask_ids: HashMap<_, _> = list
            .children
            .iter()
            .enumerate()
            .filter(|(_, layer)| layer.a("layerType") == Some("mask"))
            .map(|(index, _)| (index, features::id()))
            .collect();
        for (layer_index, node) in list.children.iter().enumerate() {
            if node.name != "DOMLayer" {
                continue;
            }
            if layers.len() >= 100 {
                return Err("Demasiadas capas XFL".into());
            }
            let kind = node.a("layerType").unwrap_or("normal");
            if kind == "folder" {
                continue;
            }
            let mut layer = Layer {
                name: node.a("name").unwrap_or("Layer").into(),
                visible: node.a("visible") != Some("false"),
                frames: vec![SharedVec::default()].into(),
                options: Default::default(),
            };
            layer.options.locked = node.a("locked") == Some("true");
            layer.options.mask_id = mask_ids.get(&layer_index).copied();
            let parent = parent_index(layer_index);
            let mut ancestor = parent;
            while let Some(index) = ancestor {
                if let Some(&folder) = folder_ids.get(&index) {
                    layer.options.folder = Some(folder);
                    break;
                }
                ancestor = parent_index(index);
            }
            if kind == "masked" {
                layer.options.masked_by = parent.and_then(|index| mask_ids.get(&index).copied());
                if layer.options.masked_by.is_none() {
                    self.warn(format!("Capa {}: no se encuentra su máscara", layer.name));
                }
            }
            if kind == "guide" || kind == "guided" {
                self.warn(format!("Capa {}: las guías se conservan como dibujo de referencia; no se reconstruye el vínculo de movimiento", layer.name));
            }
            let mut tweens = vec![];
            if let Some(frames) = node.child("frames") {
                for f in &frames.children {
                    if f.name != "DOMFrame" {
                        continue;
                    }
                    let index = integer(f, "index", 0, 9999)?;
                    let duration = integer(f, "duration", 1, 10000)?;
                    let end = index
                        .checked_add(duration)
                        .filter(|e| *e <= 10000 && duration > 0)
                        .ok_or("Duración XFL fuera de rango")?;
                    let mut marks = vec![];
                    if let Some(elements) = f.child("elements") {
                        for e in &elements.children {
                            marks.extend(self.best_element(e, index));
                        }
                    }
                    if kind == "guide" {
                        for mark in &mut marks {
                            mark.style.guide = true;
                        }
                    }
                    if self.context.starts_with("Símbolo ") && f.a("soundName").is_some() {
                        self.warn(format!(
                            "Capa {}: audio interno del símbolo omitido",
                            layer.name
                        ));
                    }
                    let mut scripts = vec![];
                    f.descendants("ActionScript", &mut scripts);
                    if !scripts.is_empty() {
                        self.warn("ActionScript y JavaScript se omiten; no se ejecuta código");
                    }
                    if f.a("tweenType").is_some_and(|s| s != "none") {
                        tweens.push((
                            index,
                            end,
                            f.a("tweenType").unwrap().to_owned(),
                            f.n("acceleration", 0.)?,
                        ));
                    }
                    if end > layer.frames.len() {
                        layer.frames.resize(end, Default::default());
                        layer.options.keys.resize(end, false);
                    }
                    layer.options.keys.resize(layer.frames.len(), false);
                    layer.options.keys[0] = true;
                    layer.options.keys[index] = true;
                    let drawing: SharedVec<_> = marks.into();
                    for i in index..end {
                        layer.frames[i] = drawing.clone();
                    }
                }
            }
            for (start, end, kind, ease) in tweens {
                if kind == "motion"
                    && end < layer.frames.len()
                    && layer.key(end)
                    && layer.frames[start].len() == layer.frames[end].len()
                {
                    let mut a = layer.frames[start].clone();
                    let mut b = layer.frames[end].clone();
                    let mut valid = true;
                    for (first, last) in a.iter_mut().zip(b.iter_mut()) {
                        if first.style.instance.as_ref().map(|i| i.symbol)
                            != last.style.instance.as_ref().map(|i| i.symbol)
                        {
                            valid = false;
                            break;
                        }
                        self.next_track += 1;
                        first.style.track = self.next_track;
                        last.style.track = self.next_track;
                        if let (Some(a), Some(b)) =
                            (first.style.instance.as_ref(), last.style.instance.as_mut())
                        {
                            b.born = a.born;
                        }
                    }
                    if valid {
                        let mut curve = Curve::default();
                        if ease > 0. {
                            curve.pace = [(1. / 3. + ease / 150.).clamp(0., 1.), 0.];
                        } else if ease < 0. {
                            curve.pace = [0., (1. / 3. - ease / 150.).clamp(0., 1.)];
                        }
                        layer.frames[start] = a.clone();
                        for f in start + 1..end {
                            layer.frames[f] = a.clone();
                        }
                        layer.frames[end] = b;
                        layer.options.tweens.push(Tween { start, end, curve });
                        self.warn("Interpolaciones clásicas importadas: transformación y aceleración; trayectorias y giros múltiples pueden variar");
                        continue;
                    }
                }
                self.warn(format!("Interpolación {kind}: se conservan las claves y exposiciones; no se reconstruye su deformación/trayectoria"));
            }
            layers.push(layer);
        }
        if layers.is_empty() {
            layers.push(Layer {
                name: "Layer".into(),
                visible: true,
                frames: vec![SharedVec::default()].into(),
                options: Default::default(),
            });
        }
        let count = layers.iter().map(|l| l.frames.len()).max().unwrap();
        for l in &mut layers {
            l.frames.resize(count, Default::default());
            l.options.keys.resize(count, false);
            l.options.keys[0] = true;
        }
        layers.reverse();
        Ok((layers.into(), folders.into()))
    }
}
fn integer(n: &Node, key: &str, default: usize, max: usize) -> Result<usize, String> {
    let v = n.n(key, default as f32)?;
    if v < 0. || v.fract() != 0. || v > max as f32 {
        Err(format!("{key} XFL fuera de rango"))
    } else {
        Ok(v as usize)
    }
}
fn color_hex(s: &str) -> Result<[u8; 4], String> {
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 || !h.is_ascii() {
        return Err("Color XFL no válido".into());
    }
    Ok([
        u8::from_str_radix(&h[0..2], 16).map_err(|e| e.to_string())?,
        u8::from_str_radix(&h[2..4], 16).map_err(|e| e.to_string())?,
        u8::from_str_radix(&h[4..6], 16).map_err(|e| e.to_string())?,
        255,
    ])
}
// Animate's lossless bitmap entries store little-endian dimensions and
// rows of ABGR pixels, optionally wrapped in length-prefixed zlib blocks.
fn decode_bitmap(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    use std::io::Read;
    let invalid = || "Datos de imagen XFL no válidos".to_owned();
    if !bytes.starts_with(&[3, 5]) {
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| invalid())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        reader.limits(limits);
        let image = reader.decode().map_err(|_| invalid())?.to_rgba8();
        let (w, h) = image.dimensions();
        return Ok((image.into_raw(), w, h));
    }
    let word = |at: usize| -> Option<usize> {
        Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?) as usize)
    };
    let stride = word(2).ok_or_else(invalid)?;
    let w = word(4).ok_or_else(invalid)?;
    let h = word(6).ok_or_else(invalid)?;
    if !(1..=4096).contains(&w)
        || !(1..=4096).contains(&h)
        || stride < w * 4
        || stride * h > 64 * 1024 * 1024
    {
        return Err(invalid());
    }
    let alpha = bytes.get(24).ok_or_else(invalid)? & 1 != 0;
    let expected = stride * h;
    let mut raw = Vec::new();
    match bytes.get(25) {
        Some(0) => raw.extend_from_slice(bytes.get(26..26 + expected).ok_or_else(invalid)?),
        Some(1) => {
            let mut compressed = Vec::new();
            let mut at = 26;
            loop {
                let n = word(at).ok_or_else(invalid)?;
                at += 2;
                if n == 0 {
                    break;
                }
                compressed.extend_from_slice(bytes.get(at..at + n).ok_or_else(invalid)?);
                at += n;
            }
            flate2::read::ZlibDecoder::new(compressed.as_slice())
                .take(expected as u64 + 1)
                .read_to_end(&mut raw)
                .map_err(|_| invalid())?;
        }
        _ => return Err(invalid()),
    }
    if raw.len() != expected {
        return Err(invalid());
    }
    let mut pixels = Vec::with_capacity(w * h * 4);
    for row in raw.chunks_exact(stride) {
        for p in row[..w * 4].chunks_exact(4) {
            let a = if alpha { p[0] } else { 255 };
            let channel = |value: u8| {
                if a == 0 {
                    0
                } else {
                    ((value as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8
                }
            };
            pixels.extend([channel(p[3]), channel(p[2]), channel(p[1]), a]);
        }
    }
    Ok((pixels, w as u32, h as u32))
}

pub fn import(bytes: &[u8], xml: &str) -> Result<Document, String> {
    let root = parse(xml)?;
    if root.name != "DOMDocument" {
        return Err("No es un DOMDocument de Animate".into());
    }
    let mut doc = Document::default();
    doc.layers.clear();
    doc.width = integer(&root, "width", 550, 16384)?.max(1) as u32;
    doc.height = integer(&root, "height", 400, 16384)?.max(1) as u32;
    let fps = root.n("frameRate", 24.)?;
    if !(1. ..=240.).contains(&fps) {
        return Err("FPS XFL fuera de rango".into());
    }
    doc.fps = fps.round() as u32;
    let mut imp = Importer {
        archive: crate::fla_zip::Archive::new(bytes)?,
        names: HashMap::new(),
        bitmaps: HashMap::new(),
        kinds: HashMap::new(),
        warnings: BTreeSet::new(),
        next_track: 1,
        context: String::new(),
        total: 0,
    };
    if imp.archive.directory_size_recovered() {
        imp.warn("Directorio ZIP de Animate con tamaño declarado incorrecto: recuperado a partir de los registros del directorio");
    }
    if fps.fract() != 0. {
        imp.warn(format!("FPS fraccionarios {fps} redondeados a {}", doc.fps));
    }
    let mut sounds = HashMap::new();
    if let Some(media) = root.child("media") {
        let mut used = 0usize;
        for item in &media.children {
            let name = item.a("name").unwrap_or("").trim();
            if item.name == "DOMSoundItem" && !name.is_empty() {
                if sounds.contains_key(name) {
                    continue;
                }
                let candidates = [
                    item.a("href").map(|href| format!("LIBRARY/{href}")),
                    item.a("href").map(str::to_owned),
                    item.a("soundDataHRef").map(|href| format!("bin/{href}")),
                ];
                let sound = candidates.into_iter().flatten().find_map(|path| {
                    let bytes = imp.archive.entry(path.as_bytes()).ok().flatten()?;
                    if used.saturating_add(bytes.len()) > 256 * 1024 * 1024 {
                        return None;
                    }
                    crate::media::imported_audio(bytes, name.into())
                });
                if let Some(asset) = sound {
                    used += asset.data.len();
                    sounds.insert(name.to_owned(), doc.extras.media.assets.len());
                    doc.extras.media.assets.push(asset);
                } else {
                    imp.warn(format!(
                        "Audio ausente, no compatible o demasiado grande: {name}"
                    ));
                }
                continue;
            }
            if item.name != "DOMBitmapItem" {
                imp.warn(format!("Recurso {} omitido: {name}", item.name));
                continue;
            }
            if name.is_empty() || imp.bitmaps.contains_key(name) {
                continue;
            }
            let candidates = [
                item.a("bitmapDataHRef").map(|href| format!("bin/{href}")),
                item.a("href").map(|href| format!("LIBRARY/{href}")),
                item.a("href").map(str::to_owned),
            ];
            let decoded = candidates.into_iter().flatten().find_map(|path| {
                imp.archive
                    .entry(path.as_bytes())
                    .ok()
                    .flatten()
                    .and_then(|bytes| decode_bitmap(&bytes).ok())
            });
            if let Some((pixels, width, height)) = decoded
                && used.saturating_add(pixels.len()) <= 256 * 1024 * 1024
            {
                used += pixels.len();
                let asset = crate::media::bitmap(pixels, width, height, name.into());
                imp.bitmaps.insert(name.into(), (asset.id, width, height));
                doc.extras.media.assets.push(asset);
            } else {
                imp.warn(format!(
                    "Imagen incrustada ausente, no compatible o demasiado grande: {name}"
                ));
            }
        }
    }
    let background = root.a("backgroundColor").map(color_hex).transpose()?;
    let mut definitions = vec![];
    let mut includes = vec![];
    root.descendants("Include", &mut includes);
    let mut budget = 0;
    let mut loaded = BTreeSet::new();
    for include in includes {
        if definitions.len() >= 500 {
            return Err("Demasiados símbolos XFL".into());
        }
        let href = include.a("href").unwrap_or("");
        let path = format!("LIBRARY/{href}");
        if !loaded.insert(path.clone()) {
            continue;
        }
        match imp.archive.entry(path.as_bytes()) {
            Ok(Some(data)) => {
                budget += data.len();
                if budget > 128 * 1024 * 1024 {
                    return Err("Biblioteca XFL demasiado grande".into());
                }
                match crate::fla_zip::xml_text(data).and_then(|s| parse(&s)) {
                    Ok(n) if n.name == "DOMSymbolItem" => {
                        let name = n.a("name").unwrap_or(href).to_owned();
                        if imp.names.contains_key(&name) {
                            imp.warn(format!("Definición de símbolo duplicada omitida: {name}"));
                            continue;
                        }
                        let id = definitions.len() as u64 + 1;
                        imp.names.insert(name.clone(), id);
                        imp.kinds
                            .insert(id, n.a("symbolType").unwrap_or("movie clip").into());
                        definitions.push((id, name, n));
                    }
                    Ok(_) => imp.warn(format!("Recurso no válido: {href}")),
                    Err(e) => imp.warn(format!("Símbolo {href} omitido: {e}")),
                }
            }
            Ok(None) => imp.warn(format!("Recurso ausente: {href}")),
            Err(e) => imp.warn(format!("Recurso {href} omitido: {e}")),
        }
    }
    for (id, name, n) in &definitions {
        imp.context = format!("Símbolo {name}");
        let (layers, folders) = match n.path("timeline/DOMTimeline") {
            Some(t) => match imp.timeline(t) {
                Ok(l) => l,
                Err(e) => {
                    imp.warn(format!("Símbolo {name} omitido: {e}"));
                    (
                        vec![Layer {
                            name: "Recurso no compatible".into(),
                            visible: true,
                            frames: vec![SharedVec::default()].into(),
                            options: Default::default(),
                        }]
                        .into(),
                        Default::default(),
                    )
                }
            },
            None => {
                imp.warn(format!("Símbolo sin línea de tiempo: {name}"));
                (
                    vec![Layer {
                        name: "Layer".into(),
                        visible: true,
                        frames: vec![SharedVec::default()].into(),
                        options: Default::default(),
                    }]
                    .into(),
                    Default::default(),
                )
            }
        };
        doc.extras.symbols.push(Symbol {
            id: *id,
            name: name.clone(),
            layers,
            folders,
            perspective: Default::default(),
            rig: Default::default(),
            library_folder: None,
            mouth_poses: Default::default(),
        });
    }
    // Traverse the dependency DAG once, not once per held frame or path.
    let graph = features::symbol_graph(&doc.extras.symbols);
    fn sanitize(
        id: u64,
        graph: &HashMap<u64, Vec<u64>>,
        active: &mut BTreeSet<u64>,
        memo: &mut HashMap<u64, usize>,
        bad: &mut BTreeSet<(u64, u64)>,
        order: &mut Vec<u64>,
    ) -> usize {
        if let Some(depth) = memo.get(&id) {
            return *depth;
        }
        active.insert(id);
        let mut depth = 1;
        for child in graph.get(&id).into_iter().flatten() {
            if active.contains(child) {
                bad.insert((id, *child));
                continue;
            }
            let child_depth = sanitize(*child, graph, active, memo, bad, order);
            if child_depth >= 8 {
                bad.insert((id, *child));
            } else {
                depth = depth.max(1 + child_depth);
            }
        }
        active.remove(&id);
        memo.insert(id, depth);
        order.push(id);
        depth
    }
    let mut bad = BTreeSet::new();
    let mut order = vec![];
    let mut memo = HashMap::new();
    for s in &doc.extras.symbols {
        sanitize(
            s.id,
            &graph,
            &mut BTreeSet::new(),
            &mut memo,
            &mut bad,
            &mut order,
        );
    }
    if !bad.is_empty() {
        imp.warn("Referencias recursivas o anidación superior a 8 símbolos omitidas");
        for s in &mut doc.extras.symbols {
            for l in &mut s.layers {
                for f in &mut l.frames {
                    if f.iter().any(|m| {
                        m.style
                            .instance
                            .as_ref()
                            .is_some_and(|i| bad.contains(&(s.id, i.symbol)))
                    }) {
                        f.retain(|m| {
                            m.style
                                .instance
                                .as_ref()
                                .is_none_or(|i| !bad.contains(&(s.id, i.symbol)))
                        });
                    }
                }
            }
        }
    }
    // Symbols use their actual vector bounds for selection and later editing.
    for id in order {
        let snapshots = doc.extras.symbols.clone();
        if let Some(s) = doc.extras.symbols.iter_mut().find(|s| s.id == id) {
            bounds(&mut s.layers, &snapshots);
        }
    }
    let timelines = root.child("timelines").ok_or("FLA sin escenas")?;
    for t in &timelines.children {
        if t.name != "DOMTimeline" {
            continue;
        }
        if doc.extras.scenes.len() >= 100 {
            return Err("Demasiadas escenas".into());
        }
        imp.context = format!("Escena {}", t.a("name").unwrap_or("Escena"));
        (doc.layers, doc.extras.folders) = imp.timeline(t)?;
        let mut markers = BTreeSet::new();
        for frame in t
            .path("layers")
            .into_iter()
            .flat_map(|layers| &layers.children)
            .filter_map(|layer| layer.child("frames"))
            .flat_map(|frames| &frames.children)
            .filter(|frame| frame.name == "DOMFrame")
        {
            if let Some(name) = frame
                .a("name")
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                let index = integer(frame, "index", 0, 9999)?;
                if index < doc.layers[0].frames.len() && name.len() <= 160 && markers.len() < 10000
                {
                    markers.insert((index, name.to_owned()));
                }
            }
        }
        doc.extras.markers = markers
            .into_iter()
            .map(|(frame, name)| crate::dialogue::Marker {
                frame,
                name,
                color: [91, 158, 255],
            })
            .collect::<Vec<_>>()
            .into();
        doc.extras.media.tracks.clear();
        let count = doc.layers[0].frames.len();
        for layer in t
            .child("layers")
            .into_iter()
            .flat_map(|layers| &layers.children)
        {
            let layer_name = layer.a("name").unwrap_or("Audio");
            let mut entries = Vec::new();
            for frame in layer
                .child("frames")
                .into_iter()
                .flat_map(|frames| &frames.children)
            {
                let Some(name) = frame.a("soundName").filter(|name| !name.is_empty()) else {
                    continue;
                };
                let Some(&asset) = sounds.get(name) else {
                    imp.warn(format!("Capa {layer_name}: audio omitido: {name}"));
                    continue;
                };
                let start = integer(frame, "index", 0, 9999)?;
                let duration = integer(frame, "duration", 1, 10000)?;
                let sync = frame.a("soundSync").unwrap_or("event");
                if sync == "stop" {
                    imp.warn(format!(
                        "Capa {layer_name}: evento de parada de audio omitido"
                    ));
                    continue;
                }
                let limit = if sync == "stream" { duration } else { count };
                let offset = frame.n("inPoint44", 0.).unwrap_or(0.).max(0.);
                let offset = (offset * doc.fps as f32 / 44100.).round() as usize;
                if frame.a("soundLoop").is_some_and(|n| n != "1")
                    || frame.child("soundEnvelope").is_some()
                {
                    imp.warn(format!(
                        "Capa {layer_name}: bucles o envolvente de audio no importados"
                    ));
                }
                entries.push((
                    &doc.extras.media.assets[asset],
                    start,
                    offset,
                    count.saturating_sub(start).min(limit),
                ));
            }
            if let Some(track) =
                crate::media::imported_sound_track(layer_name.into(), entries, doc.fps)
            {
                doc.extras.media.tracks.push(track);
            }
        }
        if let Some(color) = background.filter(|c| *c != [255, 255, 255, 255]) {
            if doc.layers.len() < 100 {
                let mark = Mark {
                    points: vec![[0., 0.], [doc.width as f32, doc.height as f32]].into(),
                    rectangle: true,
                    closed: true,
                    stroke: false,
                    fill: Some(color),
                    ..Default::default()
                };
                let count = doc.layers[0].frames.len();
                let mut options = features::LayerOptions::default();
                options.locked = true;
                options.keys = vec![false; count].into();
                options.keys[0] = true;
                doc.layers.insert(
                    0,
                    Layer {
                        name: "Fondo de Animate".into(),
                        visible: true,
                        frames: vec![vec![mark].into(); count].into(),
                        options,
                    },
                );
            } else {
                imp.warn("Fondo de color omitido: se alcanzó el límite de 100 capas");
            }
        }

        bounds(&mut doc.layers, &doc.extras.symbols);
        let scene = animation::scene_from(
            &doc,
            doc.extras.scenes.len() as u64 + 1000,
            t.a("name").unwrap_or("Escena").into(),
        );
        doc.extras.scenes.push(scene);
    }
    if doc.extras.scenes.is_empty() {
        return Err("Documento XFL sin línea de tiempo".into());
    }
    doc.layers = doc.extras.scenes[0].layers.clone();
    doc.extras.folders = doc.extras.scenes[0].folders.clone();
    doc.extras.markers = doc.extras.scenes[0].markers.clone();
    doc.extras.media.tracks = doc.extras.scenes[0].media_tracks.clone();
    imp.context.clear();
    if doc.extras.scenes.len() == 1 {
        doc.extras.scenes.clear();
    }
    let mut scripts = vec![];
    root.descendants("ActionScript", &mut scripts);
    if !scripts.is_empty() {
        imp.warn("ActionScript y JavaScript se omiten; no se ejecuta código");
    }
    doc.extras.import_warnings = imp.warnings.into_iter().collect();
    doc.extras.imported = true;
    doc.validate()?;
    Ok(doc)
}
fn bounds(layers: &mut [Layer], symbols: &[Symbol]) {
    for layer in layers {
        let mut cache: HashMap<usize, SharedVec<Mark>> = HashMap::new();
        for frame in &mut layer.frames {
            if let Some(existing) = cache.get(&frame.identity()) {
                *frame = existing.clone();
                continue;
            }
            let identity = frame.identity();
            if frame.iter().any(|m| m.style.instance.is_some()) {
                let mut updated = frame.clone();
                for m in &mut updated {
                    if let Some(i) = &m.style.instance {
                        if let Some(s) = symbols.iter().find(|s| s.id == i.symbol) {
                            let rect = s
                                .layers
                                .iter()
                                .flat_map(|l| l.frames.iter().take(1).flatten())
                                .fold(Rect::NOTHING, |r, m| {
                                    r.union(crate::vector::control_bounds(m))
                                });
                            if rect.is_finite() {
                                let r =
                                    Rect::from_min_size(rect.min, rect.size().max(Vec2::splat(1.)));
                                let mut corners = vec![
                                    r.left_top(),
                                    r.right_top(),
                                    r.right_bottom(),
                                    r.left_bottom(),
                                ];
                                for p in &mut corners {
                                    *p = rig::point(i.matrix, *p);
                                }
                                m.points = corners.into_iter().map(crate::vector::xy).collect();
                                m.rectangle = false;
                                m.closed = true;
                            }
                        }
                    }
                }
                *frame = updated;
            }
            cache.insert(identity, frame.clone());
        }
    }
}

impl crate::Editor {
    pub fn import_notice_ui(&mut self, ctx: &eframe::egui::Context) {
        if !self.import_report_open {
            return;
        }
        use crate::{
            design::{Icon, button},
            i18n::tr,
        };
        let title = tr("Importación de Animate · Compatibilidad");
        let summary = tr(
            "Se ha abierto la animación compatible. El código ActionScript/JavaScript no se ejecuta.",
        );
        let no_issues = tr("No se han detectado elementos omitidos en el contenido compatible.");
        let original = tr("Guardar creará un proyecto .ltoon y conservará el FLA original.");
        let mut open = true;
        let mut export = false;
        eframe::egui::Window::new(title)
            .id(eframe::egui::Id::unique("fla-import-report"))
            .open(&mut open)
            .default_width(520.)
            .show(ctx, |ui| {
                ui.label(summary);
                if self.doc.extras.imported {
                    ui.label(original);
                }
                ui.separator();
                eframe::egui::ScrollArea::vertical()
                    .max_height(350.)
                    .show(ui, |ui| {
                        if self.doc.extras.import_warnings.is_empty() {
                            ui.label(no_issues);
                        }
                        for message in &self.doc.extras.import_warnings {
                            ui.label(format!("• {message}"));
                        }
                    });
                ui.separator();
                export = button(
                    ui,
                    Icon::Save,
                    "Save report…",
                    "Save report as plain text",
                    false,
                    true,
                )
                .clicked();
            });
        if export {
            let source = self.path.as_deref();
            let stem = source
                .and_then(std::path::Path::file_stem)
                .map_or_else(|| "LibreToon".into(), |name| name.to_string_lossy());
            let mut dialog = rfd::FileDialog::new()
                .add_filter(tr("Text file"), &["txt"])
                .set_file_name(format!("{stem}-import-report.txt"));
            if let Some(folder) = source.and_then(std::path::Path::parent) {
                dialog = dialog.set_directory(folder);
            }
            if let Some(mut destination) = dialog.save_file() {
                if destination.extension().is_none() {
                    destination.set_extension("txt");
                }
                if !destination
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"))
                {
                    self.status = tr("Choose a .txt file for the report.").into();
                    self.import_report_open = open;
                    return;
                }
                let mut report = format!("{title}\n{summary}\n");
                if self.doc.extras.imported {
                    report.push_str(original);
                    report.push('\n');
                }
                report.push('\n');
                if self.doc.extras.import_warnings.is_empty() {
                    report.push_str(no_issues);
                    report.push('\n');
                }
                for warning in &self.doc.extras.import_warnings {
                    report.push_str("- ");
                    report.push_str(warning);
                    report.push('\n');
                }
                self.status = match std::fs::write(&destination, report) {
                    Ok(()) => format!("{} {}", tr("Import report saved"), destination.display()),
                    Err(error) => format!("{} {error}", tr("Could not save import report")),
                };
            }
        }
        self.import_report_open = open;
    }
}

#[cfg(test)]
mod bitmap_tests {
    use super::*;

    #[test]
    fn embedded_animate_bitmap_becomes_a_positioned_media_instance() {
        use std::io::Write;
        let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        zlib.write_all(&[255, 0, 0, 255, 128, 0, 64, 0]).unwrap();
        let block = zlib.finish().unwrap();
        let mut data = vec![0u8; 26];
        data[..8].copy_from_slice(&[3, 5, 8, 0, 2, 0, 1, 0]);
        data[24..26].copy_from_slice(&[1, 1]);
        data.extend_from_slice(&(block.len() as u16).to_le_bytes());
        data.extend_from_slice(&block);
        data.extend_from_slice(&[0, 0]);
        let xml = r#"<DOMDocument width="12" height="12"><media>
          <DOMBitmapItem name="photo.png" bitmapDataHRef="photo.dat"/>
        </media><timelines><DOMTimeline><layers><DOMLayer name="Art"><frames>
          <DOMFrame index="0"><elements><DOMBitmapInstance libraryItemName="photo.png">
            <matrix><Matrix tx="3" ty="5"/></matrix>
          </DOMBitmapInstance></elements></DOMFrame>
        </frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#;
        let archive = crate::fla_zip::pack(&[
            ("DOMDocument.xml", xml.as_bytes().to_vec()),
            ("bin/photo.dat", data),
        ]);
        let doc = crate::fla::decode(&archive).unwrap();
        let mark = &doc.layers[0].frames[0][0];
        let id = mark.style.media_image.unwrap();
        let (size, rgba) = doc.extras.media.pixels(id).unwrap();
        assert_eq!(size, [2, 1]);
        assert_eq!(rgba, [255, 0, 0, 255, 0, 128, 0, 128]);
        assert_eq!(crate::vector::control_bounds(mark).min, Pos2::new(3., 5.));
        let reopened = crate::fla::decode(&crate::fla::encode(&doc).unwrap()).unwrap();
        assert_eq!(reopened.extras.media.pixels(id).unwrap().1, rgba);
    }
}
