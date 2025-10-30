const sharp = require('sharp');
const fs = require('fs');
const path = require('path');
const pngToIco = require('png-to-ico').default;
const png2icons = require('png2icons');

const iconsDir = path.join(__dirname, 'src-tauri', 'icons');
const windowsSvg = fs.readFileSync(path.join(iconsDir, 'icon_windows.svg'));
const macosSvg = fs.readFileSync(path.join(iconsDir, 'icon_macos.svg'));

const windowsSizes = [
  { size: 16, name: '16x16.png' },
  { size: 32, name: '32x32.png' },
  { size: 48, name: '48x48.png' },
  { size: 64, name: '64x64.png' },
  { size: 128, name: '128x128.png' },
  { size: 256, name: '128x128@2x.png' },
  { size: 512, name: 'icon.png' },
  { size: 30, name: 'Square30x30Logo.png' },
  { size: 44, name: 'Square44x44Logo.png' },
  { size: 71, name: 'Square71x71Logo.png' },
  { size: 89, name: 'Square89x89Logo.png' },
  { size: 107, name: 'Square107x107Logo.png' },
  { size: 142, name: 'Square142x142Logo.png' },
  { size: 150, name: 'Square150x150Logo.png' },
  { size: 284, name: 'Square284x284Logo.png' },
  { size: 310, name: 'Square310x310Logo.png' },
  { size: 50, name: 'StoreLogo.png' },
];

const macosSizes = [
  { size: 16, name: 'macos_16.png' },
  { size: 32, name: 'macos_32.png' },
  { size: 64, name: 'macos_64.png' },
  { size: 128, name: 'macos_128.png' },
  { size: 256, name: 'macos_256.png' },
  { size: 512, name: 'macos_512.png' },
  { size: 1024, name: 'macos_1024.png' },
];

const iosIcons = [
  { size: 20, name: 'AppIcon-20x20@1x.png' },
  { size: 40, name: 'AppIcon-20x20@2x.png' },
  { size: 40, name: 'AppIcon-20x20@2x-1.png' },
  { size: 60, name: 'AppIcon-20x20@3x.png' },
  { size: 29, name: 'AppIcon-29x29@1x.png' },
  { size: 58, name: 'AppIcon-29x29@2x.png' },
  { size: 58, name: 'AppIcon-29x29@2x-1.png' },
  { size: 87, name: 'AppIcon-29x29@3x.png' },
  { size: 40, name: 'AppIcon-40x40@1x.png' },
  { size: 80, name: 'AppIcon-40x40@2x.png' },
  { size: 80, name: 'AppIcon-40x40@2x-1.png' },
  { size: 120, name: 'AppIcon-40x40@3x.png' },
  { size: 120, name: 'AppIcon-60x60@2x.png' },
  { size: 180, name: 'AppIcon-60x60@3x.png' },
  { size: 76, name: 'AppIcon-76x76@1x.png' },
  { size: 152, name: 'AppIcon-76x76@2x.png' },
  { size: 167, name: 'AppIcon-83.5x83.5@2x.png' },
  { size: 1024, name: 'AppIcon-512@2x.png' },
];

const androidIcons = [
  { size: 48, density: 'mdpi', name: 'ic_launcher.png' },
  { size: 48, density: 'mdpi', name: 'ic_launcher_foreground.png' },
  { size: 48, density: 'mdpi', name: 'ic_launcher_round.png' },
  { size: 72, density: 'hdpi', name: 'ic_launcher.png' },
  { size: 72, density: 'hdpi', name: 'ic_launcher_foreground.png' },
  { size: 72, density: 'hdpi', name: 'ic_launcher_round.png' },
  { size: 96, density: 'xhdpi', name: 'ic_launcher.png' },
  { size: 96, density: 'xhdpi', name: 'ic_launcher_foreground.png' },
  { size: 96, density: 'xhdpi', name: 'ic_launcher_round.png' },
  { size: 144, density: 'xxhdpi', name: 'ic_launcher.png' },
  { size: 144, density: 'xxhdpi', name: 'ic_launcher_foreground.png' },
  { size: 144, density: 'xxhdpi', name: 'ic_launcher_round.png' },
  { size: 192, density: 'xxxhdpi', name: 'ic_launcher.png' },
  { size: 192, density: 'xxxhdpi', name: 'ic_launcher_foreground.png' },
  { size: 192, density: 'xxxhdpi', name: 'ic_launcher_round.png' },
];

async function generateIcons() {
  console.log('=== FINAL ICON GENERATION (Larger, closer to boundaries) ===\n');

  console.log('Windows Icons (480px radius, transparent):');
  for (const icon of windowsSizes) {
    await sharp(windowsSvg).resize(icon.size, icon.size).png().toFile(path.join(iconsDir, icon.name));
    console.log(`  ✓ ${icon.name} (${icon.size}x${icon.size})`);
  }

  console.log('\nmacOS Icons (460px radius, with background):');
  for (const icon of macosSizes) {
    await sharp(macosSvg).resize(icon.size, icon.size).png().toFile(path.join(iconsDir, icon.name));
    console.log(`  ✓ ${icon.name} (${icon.size}x${icon.size})`);
  }

  console.log('\niOS Icons:');
  for (const icon of iosIcons) {
    await sharp(macosSvg).resize(icon.size, icon.size).png().toFile(path.join(iconsDir, 'ios', icon.name));
    console.log(`  ✓ ios/${icon.name} (${icon.size}x${icon.size})`);
  }

  console.log('\nAndroid Icons:');
  for (const icon of androidIcons) {
    await sharp(windowsSvg).resize(icon.size, icon.size).png().toFile(path.join(iconsDir, 'android', `mipmap-${icon.density}`, icon.name));
    console.log(`  ✓ android/mipmap-${icon.density}/${icon.name} (${icon.size}x${icon.size})`);
  }

  console.log('\nAdditional:');
  await sharp(macosSvg).resize(1024, 1024).png().toFile(path.join(iconsDir, 'chiral_app_icon_r.png'));
  console.log('  ✓ chiral_app_icon_r.png (1024x1024)');

  console.log('\nWindows .ico:');
  const icoFiles = ['16x16.png', '32x32.png', '48x48.png', '64x64.png', '128x128.png', '128x128@2x.png'].map(f => path.join(iconsDir, f));
  fs.writeFileSync(path.join(iconsDir, 'icon.ico'), await pngToIco(icoFiles));
  console.log('  ✓ icon.ico');

  console.log('\nmacOS .icns:');
  const icns = png2icons.createICNS(fs.readFileSync(path.join(iconsDir, 'macos_1024.png')), png2icons.BILINEAR, 0);
  fs.writeFileSync(path.join(iconsDir, 'icon.icns'), icns);
  console.log('  ✓ icon.icns');

  console.log('\n✅ ALL DONE!');
  console.log('  • Windows: 480px radius (93.75% of space), transparent, 2 orbital rings');
  console.log('  • macOS: 460px radius (89.8% of space), background, 2 orbital rings');
  console.log('  • Both: 6 evenly-spaced mesh lines, bold with halos');
}

generateIcons().catch(console.error);
