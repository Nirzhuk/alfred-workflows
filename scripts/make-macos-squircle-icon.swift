#!/usr/bin/env swift

import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

guard CommandLine.arguments.count == 3 else {
  fputs("usage: make-macos-squircle-icon.swift INPUT.png OUTPUT.png\n", stderr)
  exit(64)
}

let inputURL = URL(fileURLWithPath: CommandLine.arguments[1])
let outputURL = URL(fileURLWithPath: CommandLine.arguments[2])

guard
  let source = CGImageSourceCreateWithURL(inputURL as CFURL, nil),
  let sourceImage = CGImageSourceCreateImageAtIndex(source, 0, nil)
else {
  fputs("could not read input image\n", stderr)
  exit(1)
}

let size = max(sourceImage.width, sourceImage.height)
let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)!
let bitmapInfo = CGImageAlphaInfo.premultipliedLast.rawValue

guard
  let context = CGContext(
    data: nil,
    width: size,
    height: size,
    bitsPerComponent: 8,
    bytesPerRow: 0,
    space: colorSpace,
    bitmapInfo: bitmapInfo
  )
else {
  fputs("could not create drawing context\n", stderr)
  exit(1)
}

context.clear(CGRect(x: 0, y: 0, width: size, height: size))

// This is the platform export only. The source master stays full-bleed so
// Apple can apply the native mask in contexts that provide one.
let cornerRadius = CGFloat(size) * 0.168
let iconRect = CGRect(x: 0, y: 0, width: size, height: size)
let mask = CGPath(
  roundedRect: iconRect,
  cornerWidth: cornerRadius,
  cornerHeight: cornerRadius,
  transform: nil
)
context.addPath(mask)
context.clip()
context.draw(sourceImage, in: iconRect)

guard let outputImage = context.makeImage() else {
  fputs("could not create output image\n", stderr)
  exit(1)
}

guard
  let destination = CGImageDestinationCreateWithURL(
    outputURL as CFURL,
    UTType.png.identifier as CFString,
    1,
    nil
  )
else {
  fputs("could not create output file\n", stderr)
  exit(1)
}

CGImageDestinationAddImage(destination, outputImage, nil)
guard CGImageDestinationFinalize(destination) else {
  fputs("could not write output image\n", stderr)
  exit(1)
}
