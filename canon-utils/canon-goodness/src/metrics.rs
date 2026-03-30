#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Metrics {
    pub i: f32,
    pub e: f32,
    pub c: f32,
    pub a: f32,
    pub r: f32,
    pub p: f32,
    pub s: f32,
    pub d: f32,
    pub x: f32,
    pub b: f32,
    pub l: f32,
    pub f: f32,
    pub lambda: f32,
}

impl Metrics {
    pub fn as_array(&self) -> [f32; 13] {
        [self.i, self.e, self.c, self.a, self.r, self.p, self.s, self.d, self.x, self.b, self.l, self.f, self.lambda]
    }
}
