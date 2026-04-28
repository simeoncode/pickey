class Pickey < Formula
  desc "Automatic SSH key selection for git"
  homepage "https://github.com/simeoncode/pickey"
  version "0.3.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/simeoncode/pickey/releases/download/v#{version}/pickey-aarch64-apple-darwin"
      sha256 "6302badcf633beab24d7929eb558b644aec024d165ec25f1f67c90dd12821e7f"
    else
      url "https://github.com/simeoncode/pickey/releases/download/v#{version}/pickey-x86_64-apple-darwin"
      sha256 "69ddff358df3e3a557864c9799632b3f19691331bfaa4669162a785135eed1be"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/simeoncode/pickey/releases/download/v#{version}/pickey-aarch64-unknown-linux-musl"
      sha256 "6473abacbb494308ce83251e11aa046f0583f348494412e81e1a1431066ae601"
    else
      url "https://github.com/simeoncode/pickey/releases/download/v#{version}/pickey-x86_64-unknown-linux-musl"
      sha256 "b59bfbb4ad058461ffe96e7fda9b8e1591b834a01fb1f423c38a3ffbb460a03c"
    end
  end

  def install
    binary = Dir["pickey-*"].first
    chmod 0755, binary
    bin.install binary => "pickey"
  end

  test do
    assert_match "Automatic SSH key selection for git", shell_output("#{bin}/pickey --help")
  end
end
