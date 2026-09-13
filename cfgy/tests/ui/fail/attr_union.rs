use cfgy::FromConfig;

#[derive(FromConfig)]
union Bits {
    int: u32,
    float: f32,
}

fn main() {}
