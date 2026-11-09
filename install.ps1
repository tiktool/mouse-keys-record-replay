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
    $releases = "https://github.com/$repo/releases"
    $api = "https://api.github.com/repos/$repo/releases/latest"
    $assetsApi = "https://api.github.com/repos/$repo/releases/assets"
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

    # GitHub's download edge can answer 504 for a while after a release, so
    # every request retries, and a failing /latest/download/ URL falls back to
    # the versioned one.
    function Invoke-Retry([scriptblock]$Request) {
        for ($i = 1; ; $i++) {
            try { return & $Request } catch {
                if ($i -ge 5) { throw }
                Start-Sleep -Seconds 3
            }
        }
    }
    $state = @{ Tag = $null; Release = $null; Tried = $false }
    # The release JSON from the API, fetched at most once: it names the tag for
    # the versioned URL and the asset ids for the API download.
    function Get-Release {
        if (-not $state.Tried) {
            $state.Tried = $true
            try { $state.Release = Invoke-Retry { Invoke-RestMethod -Uri $api -TimeoutSec 30 } } catch { }
        }
        return $state.Release
    }
    function Get-ReleaseTag {
        if ($state.Tag) { return $state.Tag }
        $t = $null
        $rel = Get-Release
        if ($rel) { $t = $rel.tag_name }
        if (-not $t) {
            # /releases/latest redirects to /releases/tag/<tag>.
            try {
                $r = Invoke-Retry { Invoke-WebRequest -UseBasicParsing -Method Head -Uri "$releases/latest" -TimeoutSec 30 }
                $u = if ($r.BaseResponse.ResponseUri) { $r.BaseResponse.ResponseUri } else { $r.BaseResponse.RequestMessage.RequestUri }
                $t = ([string]$u).TrimEnd('/').Split('/')[-1]
            } catch { }
        }
        if ($t -notmatch '^v[0-9]') { return $null }
        $state.Tag = $t
        return $t
    }
    # /latest/download/, then the versioned URL, then the API asset URL (served
    # when the download edge answers 504).
    function Save-ReleaseFile([string]$Name, [string]$OutFile) {
        try {
            Invoke-Retry { Invoke-WebRequest -UseBasicParsing -Uri "$releases/latest/download/$Name" -OutFile $OutFile -TimeoutSec 120 }
            return
        } catch { }
        $t = Get-ReleaseTag
        if ($t) {
            try {
                Invoke-Retry { Invoke-WebRequest -UseBasicParsing -Uri "$releases/download/$t/$Name" -OutFile $OutFile -TimeoutSec 120 }
                return
            } catch { }
        }
        $id = $null
        $rel = Get-Release
        if ($rel) { $id = ($rel.assets | Where-Object { $_.name -eq $Name } | Select-Object -First 1).id }
        if ("$id" -notmatch '^[0-9]+$') { throw "Could not download $Name from GitHub. Try again in a few minutes." }
        Invoke-Retry { Invoke-WebRequest -UseBasicParsing -Uri "$assetsApi/$id" -Headers @{ Accept = 'application/octet-stream' } -OutFile $OutFile -TimeoutSec 120 }
    }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("mkrr-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $file = Join-Path $tmp $asset
        Write-Host "Downloading $asset"
        Save-ReleaseFile $asset $file
        Save-ReleaseFile "$asset.sig" "$file.sig"

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
