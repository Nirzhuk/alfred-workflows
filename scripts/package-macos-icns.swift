#!/usr/bin/env swift

import Foundation

guard CommandLine.arguments.count == 3 else {
  fputs("usage: package-macos-icns.swift ICONSET_DIR OUTPUT.icns\n", stderr)
  exit(64)
}

let iconsetURL = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let outputURL = URL(fileURLWithPath: CommandLine.arguments[2])

// PNG-backed ICNS representations. The 2x entries keep the icon crisp in
// Finder and the Dock while the unrounded 1024px master remains untouched.
let representations: [(type: String, filename: String)] = [
  ("ic11", "icon_16x16@2x.png"),
  ("ic12", "icon_32x32@2x.png"),
  ("ic07", "icon_128x128.png"),
  ("ic13", "icon_128x128@2x.png"),
  ("ic08", "icon_256x256.png"),
  ("ic14", "icon_256x256@2x.png"),
  ("ic09", "icon_512x512.png"),
  ("ic10", "icon_512x512@2x.png"),
]

func bigEndian(_ value: UInt32) -> [UInt8] {
  [
    UInt8((value >> 24) & 0xff),
    UInt8((value >> 16) & 0xff),
    UInt8((value >> 8) & 0xff),
    UInt8(value & 0xff),
  ]
}

var chunks = Data()
for representation in representations {
  let imageURL = iconsetURL.appendingPathComponent(representation.filename)
  guard let image = try? Data(contentsOf: imageURL) else {
    fputs("could not read \(imageURL.path)\n", stderr)
    exit(1)
  }

  let length = UInt32(image.count + 8)
  chunks.append(contentsOf: representation.type.utf8)
  chunks.append(contentsOf: bigEndian(length))
  chunks.append(image)
}

var icns = Data("icns".utf8)
icns.append(contentsOf: bigEndian(UInt32(chunks.count + 8)))
icns.append(chunks)

do {
  try icns.write(to: outputURL, options: .atomic)
} catch {
  fputs("could not write \(outputURL.path): \(error)\n", stderr)
  exit(1)
}
