// Writes a HEIC (or AVIF) with the given orientation using macOS ImageIO, which stores it as
// an `irot`/`imir` transform plus Exif. Generates tests/fixtures/rotated.heic (see its README).
// usage: heic_rotated <src.png> <out.heic|avif> <orientation 1-8>
import Foundation
import ImageIO
let a = CommandLine.arguments
let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: a[1]) as CFURL, nil)!
let img = CGImageSourceCreateImageAtIndex(src, 0, nil)!
let type = a[2].hasSuffix("avif") ? "public.avif" : "public.heic"
let dst = CGImageDestinationCreateWithURL(URL(fileURLWithPath: a[2]) as CFURL, type as CFString, 1, nil)!
let props: [CFString: Any] = [kCGImagePropertyOrientation: Int(a[3])!, kCGImageDestinationLossyCompressionQuality: 0.8,
  kCGImagePropertyExifDictionary: [kCGImagePropertyExifUserComment: "synthetic fixture"]]
CGImageDestinationAddImage(dst, img, props as CFDictionary)
print(CGImageDestinationFinalize(dst))
