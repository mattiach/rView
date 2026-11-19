"""Download the third-party fixtures of core/tests/fixtures (not in the repository).

Standard library only. Every file is checked against its SHA-256; nothing is kept on a mismatch.
Usage:  python fetch_fixtures.py        (from this folder; then gen_fixtures.py for the generated ones)
"""
import hashlib
import os
import urllib.request

OUT = os.path.dirname(os.path.abspath(__file__))
JXL = "https://raw.githubusercontent.com/libjxl/conformance/b1d0f990b03e57bf6d137c365cd5dc8b470b9191/"
DNG = "https://raw.pixls.us/getfile.php/7280/nice/Sigma%20-%20fp%20-%208bit%20(16:9).DNG"

FILES = {
    "still.jxl": (JXL + "testcases/grayscale/input.jxl", "78fbbba852e99946d187dcf0bcbd7fb0e7c22be2f0852523aaae6ed91e7e3c39"),
    "still.jxl.ref.png": (JXL + "testcases/grayscale/ref.png", "9d8d45dc6158ba173694f9e3075242c4a1e941649ed0eb9538479f83c8fed63f"),
    "oriented.jxl": (JXL + "testcases/bench_oriented_brg/input.jxl", "e223fed907c6238622b2b6ec1c80609050c9d2db4d759cf3ca6f0db304cbb82a"),
    "oriented.jxl.ref.png": (JXL + "testcases/bench_oriented_brg/ref.png", "908d2bc9656a824b69071484e15a81b62e2be67dde2ac87708747222c0949f7f"),
    "anim.jxl": (JXL + "testcases/animation_newtons_cradle/input.jxl", "35a13d365f5eb88c3249afe073b7d8b0174d74e38f7edaa8805ca8ab46a59ab4"),
    "anim.jxl.ref.gif": (JXL + "testcases/animation_newtons_cradle/ref.gif", "47f6bcd67d0fdff8074e256a034ae7cd2693613df16ccf529e48d93b62b424f0"),
    "anim.jxl.test.json": (JXL + "testcases/animation_newtons_cradle/test.json", "003a5b4c4df09970c5fdbf4d67ff1d3141917194a322e5adddce6d249d48fda3"),
    "progressive.jxl": (JXL + "testcases/progressive/input.jxl", "bb4722ab780dce609a7b007fe7fe235962b351691f60ffb2cde7b4808c8601e1"),
    "LICENSE-libjxl-conformance.txt": (JXL + "LICENSE", "8405932022a556380c2d8c272eff154a923feb197233f348ce5f7334fb0a5ede"),
    "photo.dng": (DNG, "80ae0b8fdce3f58286fc194513c6ec0080ab07caca753408b00ca72c90e5ad0c"),
}

for name, (url, sha) in FILES.items():
    dst = os.path.join(OUT, name)
    if os.path.exists(dst) and hashlib.sha256(open(dst, "rb").read()).hexdigest() == sha:
        print(f"{name}: present")
        continue
    req = urllib.request.Request(url, headers={"User-Agent": "fixture-fetch"})
    data = urllib.request.urlopen(req, timeout=60).read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha:
        print(f"{name}: SHA-256 MISMATCH ({got}), not saved")
        continue
    with open(dst, "wb") as f:
        f.write(data)
    print(f"{name}: {len(data)} bytes")
