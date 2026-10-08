// SPDX-License-Identifier: Apache-2.0
pub(super) fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let mut bytes = input.to_vec();
    bytes.push(0x80);
    while bytes.len() % 64 != 56 {
        bytes.push(0)
    }
    bytes.extend((input.len() as u64 * 8).to_le_bytes());
    let mut state = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for block in bytes.chunks_exact(64) {
        let mut words = [0u32; 16];
        for (i, b) in block.chunks_exact(4).enumerate() {
            words[i] = u32::from_le_bytes(b.try_into().unwrap())
        }
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (f, g) = if i < 16 {
                ((b & c) | (!b & d), i)
            } else if i < 32 {
                ((d & b) | (!d & c), (5 * i + 1) % 16)
            } else if i < 48 {
                (b ^ c ^ d, (3 * i + 5) % 16)
            } else {
                (c ^ (b | !d), (7 * i) % 16)
            };
            let k = ((f64::sin((i + 1) as f64).abs() * 4294967296.0).floor()) as u32;
            let next = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(k)
                    .wrapping_add(words[g])
                    .rotate_left(S[i]),
            );
            a = d;
            d = c;
            c = b;
            b = next
        }
        for (i, x) in [a, b, c, d].into_iter().enumerate() {
            state[i] = state[i].wrapping_add(x)
        }
    }
    let mut out = [0u8; 16];
    for (i, x) in state.into_iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_le_bytes())
    }
    out
}
