# Installs the latest Mouse Keys Record Replay release on Windows.
#   irm https://mouserec.com/install.ps1 | iex
# The download is checked against the release signature (Ed25519) before it
# is installed. Set $env:MKRR_NO_LAUNCH = '1' to install without starting it.
& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'tiktool/mouse-keys-record-replay'
    $asset = 'mouse-keys-record-replay-windows-x64.exe'
    $base = "https://github.com/$repo/releases/latest/download"
    $publicKey = 'uNFBmOjrwZCgQiQi+kemAO7kwutgcHyjkMdzTggxd4s='
    $dir = Join-Path $env:LOCALAPPDATA 'Programs\MouseKeysRecordReplay'
    $target = Join-Path $dir 'mouse-keys-record-replay.exe'

    if (-not ('MkrrEd25519' -as [type])) {
        Add-Type -ReferencedAssemblies System.Numerics -TypeDefinition @'
using System;
using System.Linq;
using System.Numerics;
using System.Security.Cryptography;

public static class MkrrEd25519 {
    static readonly BigInteger P = BigInteger.Pow(2, 255) - 19;
    static readonly BigInteger L = BigInteger.Pow(2, 252) + BigInteger.Parse("27742317777372353535851937790883648493");
    static readonly BigInteger D = Mod(-121665 * Inv(121666));
    static readonly BigInteger I = BigInteger.ModPow(2, (P - 1) / 4, P);

    static BigInteger Mod(BigInteger a) { BigInteger r = a % P; return r.Sign < 0 ? r + P : r; }
    static BigInteger Inv(BigInteger a) { return BigInteger.ModPow(Mod(a), P - 2, P); }
    static BigInteger Le(byte[] b) { return new BigInteger(b.Concat(new byte[] { 0 }).ToArray()); }

    static BigInteger[] Add(BigInteger[] p, BigInteger[] q) {
        BigInteger a = Mod((p[1] - p[0]) * (q[1] - q[0]));
        BigInteger b = Mod((p[1] + p[0]) * (q[1] + q[0]));
        BigInteger c = Mod(p[3] * 2 * D * q[3]);
        BigInteger d = Mod(p[2] * 2 * q[2]);
        BigInteger e = b - a, f = d - c, g = d + c, h = b + a;
        return new[] { Mod(e * f), Mod(g * h), Mod(f * g), Mod(e * h) };
    }

    static BigInteger[] Mul(BigInteger s, BigInteger[] p) {
        BigInteger[] r = { 0, 1, 1, 0 };
        for (int i = 255; i >= 0; i--) {
            r = Add(r, r);
            if (!(s >> i).IsEven) r = Add(r, p);
        }
        return r;
    }

    static BigInteger[] Decode(byte[] s) {
        byte[] b = (byte[])s.Clone();
        int sign = b[31] >> 7;
        b[31] &= 0x7f;
        BigInteger y = Le(b);
        if (y >= P) return null;
        BigInteger y2 = Mod(y * y);
        BigInteger x2 = Mod((y2 - 1) * Inv(D * y2 + 1));
        BigInteger x = BigInteger.ModPow(x2, (P + 3) / 8, P);
        if (Mod(x * x - x2) != 0) x = Mod(x * I);
        if (Mod(x * x - x2) != 0) return null;
        if (x.IsZero && sign == 1) return null;
        if ((int)(x % 2) != sign) x = P - x;
        return new[] { x, y, BigInteger.One, Mod(x * y) };
    }

    static byte[] Encode(BigInteger[] p) {
        BigInteger zi = Inv(p[2]);
        BigInteger x = Mod(p[0] * zi), y = Mod(p[1] * zi);
        byte[] raw = y.ToByteArray();
        byte[] o = new byte[32];
        Array.Copy(raw, o, Math.Min(32, raw.Length));
        if (!x.IsEven) o[31] |= 0x80;
        return o;
    }

    public static bool Verify(byte[] pub, byte[] msg, byte[] sig) {
        if (pub.Length != 32 || sig.Length != 64) return false;
        BigInteger[] a = Decode(pub);
        if (a == null) return false;
        byte[] r = sig.Take(32).ToArray();
        BigInteger s = Le(sig.Skip(32).ToArray());
        if (s >= L) return false;
        byte[] h;
        using (SHA512 sha = SHA512.Create()) h = sha.ComputeHash(r.Concat(pub).Concat(msg).ToArray());
        BigInteger k = Le(h) % L;
        byte[] by = new byte[32];
        Array.Copy((Mod(4 * Inv(5))).ToByteArray(), by, 32);
        BigInteger[] bp = Decode(by);
        BigInteger[] negA = { Mod(-a[0]), a[1], a[2], Mod(-a[3]) };
        return Encode(Add(Mul(s, bp), Mul(k, negA))).SequenceEqual(r);
    }
}
'@
    }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("mkrr-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $file = Join-Path $tmp $asset
        Write-Host "Downloading $asset"
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $file
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset.sig" -OutFile "$file.sig"

        $ok = [MkrrEd25519]::Verify([Convert]::FromBase64String($publicKey), [IO.File]::ReadAllBytes($file), [IO.File]::ReadAllBytes("$file.sig"))
        if (-not $ok) { throw 'The download failed its signature check. Nothing was installed.' }
        Write-Host 'Signature OK'

        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        # A running copy keeps its file open; Windows still allows renaming it.
        $old = "$target.old"
        Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $target) { Rename-Item -LiteralPath $target -NewName (Split-Path $old -Leaf) }
        Copy-Item -LiteralPath $file -Destination $target
        Write-Host "Installed to $target"

        if ($env:MKRR_NO_LAUNCH -ne '1') { Start-Process -FilePath $target }
        Write-Host 'Done. Press Ctrl+Alt+Shift+F1 to record.'
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }
}
