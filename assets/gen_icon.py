"""生成 桌面便利贴 的应用图标（便签 + 图钉），输出多尺寸 .ico。

设计要点：
- 暖黄渐变便签纸 + 右下角折角（dog-ear）
- 顶部红色图钉，呼应界面里的 📌
- 三条"文字线"，首条更深当标题
- 柔和投影 + 高光，16px 下仍能认出"黄块 + 红点"
"""

from PIL import Image, ImageDraw, ImageFilter

import os
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "app.ico")
S = 1024  # 基准画布


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def vertical_gradient(size, top, bottom):
    img = Image.new("RGB", (1, size))
    px = img.load()
    for y in range(size):
        px[0, y] = lerp(top, bottom, y / max(size - 1, 1))
    return img.resize((size, size), Image.NEAREST)


def rounded_mask(size, radius, box):
    m = Image.new("L", (size, size), 0)
    ImageDraw.Draw(m).rounded_rectangle(box, radius=radius, fill=255)
    return m


def build():
    # ---------- 画布 ----------
    shadow_layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow_layer)
    note_box = (140, 190, 900, 950)          # 便签主体
    radius = 78
    # 投影
    sd.rounded_rectangle(
        (note_box[0] + 18, note_box[1] + 40, note_box[2] + 18, note_box[3] + 40),
        radius=radius, fill=(60, 40, 0, 95),
    )
    shadow_layer = shadow_layer.filter(ImageFilter.GaussianBlur(34))

    # ---------- 便签纸 ----------
    grad = vertical_gradient(S, (255, 245, 176), (255, 205, 66))   # #FFF5B0 -> #FFCD42
    note = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    note.paste(grad, (0, 0))
    mask = rounded_mask(S, radius, note_box)
    # 折角：从遮罩里切掉右下三角（L 模式填 0 才会真的去掉）
    ImageDraw.Draw(mask).polygon(
        [(note_box[2] - 170, note_box[3]), (note_box[2], note_box[3] - 170), (note_box[2], note_box[3])],
        fill=0,
    )
    note.putalpha(mask)

    nd = ImageDraw.Draw(note, "RGBA")
    fold = [
        (note_box[2] - 170, note_box[3] - 0),
        (note_box[2] - 0, note_box[3] - 170),
        (note_box[2] - 170, note_box[3] - 170),
    ]
    # 折页三角（压在切口上方，浅米色带阴影）
    nd.polygon(
        [(note_box[2] - 170, note_box[3]), (note_box[2], note_box[3] - 170), (note_box[2] - 170, note_box[3] - 170)],
        fill=(255, 246, 200, 255),
    )
    nd.line(
        [(note_box[2] - 170, note_box[3]), (note_box[2] - 170, note_box[3] - 170), (note_box[2], note_box[3] - 170)],
        fill=(230, 190, 70, 255), width=6,
    )

    # 内描边（让纸更有厚度）
    nd.rounded_rectangle(note_box, radius=radius, outline=(255, 255, 255, 130), width=7)

    # ---------- 文字线 ----------
    brown = (93, 64, 55)
    bars = [
        (232, 400, 810, 456, 0.62),   # 标题（更深）
        (232, 512, 760, 556, 0.34),
        (232, 610, 640, 654, 0.26),
    ]
    for x1, y1, x2, y2, alpha in bars:
        nd.rounded_rectangle((x1, y1, x2, y2), radius=(y2 - y1) // 2,
                             fill=brown + (int(255 * alpha),))

    # 完成勾（右下，压在文字线上方，绿色）
    ck = [(560, 720), (640, 800), (830, 640)]
    nd.line(ck, fill=(76, 175, 80, 255), width=44, joint="curve")
    for pt in (ck[0], ck[-1]):
        nd.ellipse([pt[0] - 22, pt[1] - 22, pt[0] + 22, pt[1] + 22], fill=(76, 175, 80, 255))

    # ---------- 图钉 ----------
    cx = 520
    # 针
    nd.rounded_rectangle((cx - 13, 30, cx + 13, 250), radius=13, fill=(144, 164, 174, 255))
    nd.rounded_rectangle((cx - 5, 34, cx + 1, 246), radius=4, fill=(207, 222, 230, 255))
    # 钉头
    nd.ellipse([cx - 108, 46, cx + 108, 262], fill=(198, 40, 40, 255))      # 深红底
    nd.ellipse([cx - 96, 40, cx + 96, 248], fill=(229, 57, 53, 255))        # 主体红
    nd.ellipse([cx - 58, 66, cx + 26, 150], fill=(255, 138, 128, 190))      # 高光
    nd.ellipse([cx - 26, 82, cx + 6, 114], fill=(255, 205, 200, 220))       # 高光核心
    # 钉头与纸的接触阴影
    sh = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(sh).ellipse([cx - 96, 210, cx + 96, 300], fill=(120, 80, 0, 110))
    sh = sh.filter(ImageFilter.GaussianBlur(18))
    shadow_layer = Image.alpha_composite(shadow_layer, sh)

    # ---------- 合成 ----------
    return Image.alpha_composite(shadow_layer, note)


def main():
    base = build()
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    # Pillow 按 sizes 逐档缩放主图；再传 append_images 反而只产出一个尺寸
    base.save(OUT, format="ICO", sizes=[(s, s) for s in sizes])
    # 同时导出一张 256 预览图供检查
    base.resize((256, 256), Image.LANCZOS).save(OUT.replace(".ico", "_preview.png"))
    print("written:", OUT)


if __name__ == "__main__":
    main()
