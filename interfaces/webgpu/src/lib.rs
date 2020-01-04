// Copyright (C) 2020  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! WebGPU.

#![deny(intra_doc_link_resolution_failure)]
#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};

pub mod ffi;

/// Defined in the "ImageBitmap and animations" standard.
///
/// https://html.spec.whatwg.org/multipage/imagebitmap-and-animations.html#imagebitmap
pub struct ImageBitmap {

}

include!(concat!(env!("OUT_DIR"), "/webgpu.rs"));
