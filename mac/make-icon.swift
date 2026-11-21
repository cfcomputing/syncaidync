import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

guard CommandLine.arguments.count == 2,
      let context = CGContext(data: nil, width: 1024, height: 1024, bitsPerComponent: 8,
                              bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
else { exit(1) }

func color(_ rgb: UInt32) -> CGColor {
    CGColor(red: CGFloat((rgb >> 16) & 255) / 255, green: CGFloat((rgb >> 8) & 255) / 255,
            blue: CGFloat(rgb & 255) / 255, alpha: 1)
}

context.translateBy(x: 0, y: 1024)
context.scaleBy(x: 1, y: -1)
context.setFillColor(color(0x0b0716))
context.addPath(CGPath(roundedRect: CGRect(x: 64, y: 64, width: 896, height: 896),
                       cornerWidth: 224, cornerHeight: 224, transform: nil))
context.fillPath()

context.setLineWidth(60)
context.setLineCap(.round)
context.setLineJoin(.round)
let center = CGPoint(x: 512, y: 512)
for start in [-25.0, 155.0] {
    let angle = start * .pi / 180
    context.move(to: CGPoint(x: 512 + 290 * cos(angle), y: 512 + 290 * sin(angle)))
    context.addArc(center: center, radius: 290, startAngle: angle,
                   endAngle: (start + 140) * .pi / 180, clockwise: false)
}
context.replacePathWithStrokedPath()
context.saveGState()
context.clip()
if let gradient = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(),
                             colors: [color(0xa78bfa), color(0x818cf8)] as CFArray,
                             locations: [0, 1]) {
    context.drawLinearGradient(gradient, start: CGPoint(x: 307, y: 307), end: CGPoint(x: 717, y: 717),
                               options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
}
context.restoreGState()

context.setStrokeColor(color(0xe8e8ed))
context.move(to: CGPoint(x: 391, y: 415))
context.addLine(to: CGPoint(x: 488, y: 512))
context.addLine(to: CGPoint(x: 391, y: 609))
context.move(to: CGPoint(x: 536, y: 609))
context.addLine(to: CGPoint(x: 633, y: 609))
context.strokePath()

let url = URL(fileURLWithPath: CommandLine.arguments[1])
guard let image = context.makeImage(),
      let output = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)
else { exit(1) }
CGImageDestinationAddImage(output, image, nil)
if !CGImageDestinationFinalize(output) { exit(1) }
