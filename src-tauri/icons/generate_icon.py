# Create a minimal valid ICO file
import struct

# ICO file header (6 bytes)
ico_header = struct.pack('<HHH', 0, 1, 1)  # Reserved, Type=1 (ICO), Count=1

# ICO directory entry (16 bytes)
width = 32
height = 32
colors = 0  # 0 = PNG/BMP
reserved = 0
planes = 1
bit_count = 32
bytes_in_res = 40 + width * height * 4  # BMP header + pixels
image_offset = 6 + 16  # After header + directory

directory = struct.pack('<BBBBHHII',
    width, height, colors, reserved,
    planes, bit_count,
    bytes_in_res, image_offset)

# BMP info header (40 bytes)
bmp_header = struct.pack('<IIIHHIIIIII',
    40,  # biSize
    width, height * 2,  # biWidth, biHeight (double for ICO)
    1,  # biPlanes
    32,  # biBitCount
    0,  # biCompression
    width * height * 4,  # biSizeImage
    0, 0, 0, 0)  # resolution, colors, important colors

# Pixel data (simple blue color)
pixels = b'\x00\x00\xFF\xFF' * (width * height)  # BGRA format (blue with full alpha)

# Combine all parts
ico_data = ico_header + directory + bmp_header + pixels

with open('icon.ico', 'wb') as f:
    f.write(ico_data)

print('icon.ico created successfully')
