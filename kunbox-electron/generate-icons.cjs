const sharp = require('sharp');
const toIco = require('to-ico');
const fs = require('fs');
const path = require('path');

const buildDir = path.join(__dirname, 'build');
const svgPath = path.join(buildDir, 'icon.svg');

async function generateIcons() {
  console.log('Generating icons...');
  
  // Read SVG
  const svgBuffer = fs.readFileSync(svgPath);
  
  // Generate PNG at 512x512
  const png512 = await sharp(svgBuffer)
    .resize(512, 512)
    .png()
    .toBuffer();
  
  fs.writeFileSync(path.join(buildDir, 'icon.png'), png512);
  console.log('Created icon.png (512x512)');
  
  // Generate multiple sizes for ICO
  const icoSizes = [16, 32, 48, 64, 128, 256];
  const pngBuffers = [];
  
  for (const size of icoSizes) {
    const pngBuffer = await sharp(svgBuffer)
      .resize(size, size)
      .png()
      .toBuffer();
    pngBuffers.push(pngBuffer);
  }
  
  // Generate ICO from PNG buffers
  const icoBuffer = await toIco(pngBuffers);
  fs.writeFileSync(path.join(buildDir, 'icon.ico'), icoBuffer);
  console.log('Created icon.ico');
  
  // Create icons directory for Linux
  const iconsDir = path.join(buildDir, 'icons');
  if (!fs.existsSync(iconsDir)) {
    fs.mkdirSync(iconsDir);
  }
  
  // Generate multiple sizes for Linux
  const linuxSizes = [16, 24, 32, 48, 64, 128, 256, 512];
  for (const size of linuxSizes) {
    const pngBuffer = await sharp(svgBuffer)
      .resize(size, size)
      .png()
      .toBuffer();
    fs.writeFileSync(path.join(iconsDir, `${size}x${size}.png`), pngBuffer);
    console.log(`Created icons/${size}x${size}.png`);
  }
  
  console.log('Done!');
}

generateIcons().catch(console.error);
