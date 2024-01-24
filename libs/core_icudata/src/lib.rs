#[repr(C, align(16))]
struct IcuData<const LENGTH: usize>([u8; LENGTH]);

const LENGTH: usize = include_bytes!("icudtl.dat").len();
static ICU_DATA_RAW: IcuData<LENGTH> = IcuData(*include_bytes!("icudtl.dat"));

/// Raw ICU data.
pub static ICU_DATA: &[u8] = &ICU_DATA_RAW.0;
