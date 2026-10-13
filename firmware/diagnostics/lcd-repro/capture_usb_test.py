import unittest
from pathlib import Path
import zlib
import capture_usb as capture

SOURCE=Path(__file__).resolve().parents[2]/"src/lcd.c"


def transcript(payloads):
    lines=["LCDCAP BEGIN count=3"]
    for frame,payload in zip(capture.FRAME_IDS,payloads):
        lines.append(f"LCDCAP FRAME frame={frame} bytes=25600 crc={zlib.crc32(payload):08x} "
                     "captured_us=100 copy_us=2 source=20001000 request=5000000 phase=wifi_before")
        for offset in range(0,len(payload),256):
            lines.append(f"LCDCAP DATA frame={frame} offset={offset} hex={payload[offset:offset+256].hex()}")
        lines.append(f"LCDCAP END frame={frame}")
    lines.append("LCDCAP DONE count=3")
    return lines


def parse(lines):
    parser=capture.CaptureParser()
    for line in lines:parser.feed(line)
    return parser


class CaptureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):cls.expected=capture.independent_expected(SOURCE)

    def test_independent_colors_stride_and_tail(self):
        e=self.expected
        self.assertEqual(len(e),25600)
        self.assertEqual(zlib.crc32(e),0x20ac31e9)
        self.assertEqual(e[25276:25278],bytes.fromhex("ff e0"))
        self.assertEqual(e[25170:25172],bytes.fromhex("f8 1f"))
        self.assertEqual(e[25064:25066],bytes.fromhex("07 ff"))
        self.assertEqual(e[320:324],bytes.fromhex("00 00 ff ff"))
        self.assertEqual(e[636:640],bytes.fromhex("ff ff 00 00"))
        self.assertEqual(e[25598:],b"\0\0")

    def test_full_batch_is_exact_bytes(self):
        p=parse(transcript([self.expected]*3))
        self.assertTrue(p.complete)
        self.assertEqual([int(s.metadata["frame"]) for s in p.frames],[1,2,7])
        self.assertTrue(all(s.pixels==self.expected for s in p.frames))

    def test_correct_transport_crc_does_not_hide_wrong_ram(self):
        wrong=bytearray(self.expected);wrong[25599]^=1
        p=parse(transcript([self.expected,bytes(wrong),self.expected]))
        self.assertTrue(p.complete)
        d=capture.difference(p.frames[1].pixels,self.expected)
        self.assertEqual(d["different_bytes"],1)
        self.assertEqual((d["first_offset"],d["first_wire_row"],d["first_wire_column"]),(25599,79,159))

    def test_swapped_endianness_is_detected(self):
        wrong=bytearray(self.expected)
        for i in range(0,len(wrong),2):wrong[i],wrong[i+1]=wrong[i+1],wrong[i]
        p=parse(transcript([bytes(wrong)]*3))
        self.assertGreater(capture.difference(p.frames[0].pixels,self.expected)["different_bytes"],0)

    def test_wrong_offset_is_rejected(self):
        lines=transcript([self.expected]*3)
        lines[2]=lines[2].replace("offset=0 ","offset=1 ")
        with self.assertRaisesRegex(ValueError,"non-contiguous"):parse(lines)

    def test_missing_tail_is_rejected(self):
        with self.assertRaisesRegex(ValueError,"incomplete snapshot"):parse(transcript([self.expected[:-1]]*3))

    def test_duplicate_chunk_is_rejected(self):
        lines=transcript([self.expected]*3);lines.insert(3,lines[2])
        with self.assertRaisesRegex(ValueError,"non-contiguous"):parse(lines)

    def test_usb_corruption_crc_is_rejected(self):
        lines=transcript([self.expected]*3)
        lines[2]=lines[2].replace("hex=00","hex=01",1)
        with self.assertRaisesRegex(ValueError,"CRC mismatch"):parse(lines)

    def test_inverse_image_primary_colors_and_border(self):
        rgb=capture.undo_lcd_transform(self.expected)
        def pixel(x,y):return rgb[3*(160*y+x):3*(160*y+x)+3]
        self.assertEqual(pixel(1,1),bytes((255,0,0)))
        self.assertEqual(pixel(54,1),bytes((0,255,0)))
        self.assertEqual(pixel(107,1),bytes((0,0,255)))
        self.assertEqual(pixel(0,0),bytes((255,255,255)))
        self.assertEqual(pixel(10,40),bytes((0,0,0)))


if __name__=="__main__":unittest.main()
