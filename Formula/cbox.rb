class Cbox < Formula
  desc "OS-level sandboxing for AI agents and arbitrary commands"
  homepage "https://github.com/borngraced/cbox"
  version "0.2.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/borngraced/cbox/releases/download/v#{version}/cbox-aarch64-macos.tar.gz"
      sha256 "2df1b77c42d678eec0a889890e46c6aad3960f89b2c39409343e636d38a39b5a"
    else
      url "https://github.com/borngraced/cbox/releases/download/v#{version}/cbox-x86_64-macos.tar.gz"
      sha256 "285baa9346d28ef24de142ee17b308a9a91517841d2138118e59c4418b2facbf"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/borngraced/cbox/releases/download/v#{version}/cbox-aarch64-linux.tar.gz"
      sha256 "dc8df77caf7fd1d6e4780599af6c5cab2465a881d1b552c2f29f7880258bb8ed"
    else
      url "https://github.com/borngraced/cbox/releases/download/v#{version}/cbox-x86_64-linux.tar.gz"
      sha256 "1cdd965f994de7c721baaccced64083ae37a2356899f69f0aa5fa1ca0209db74"
    end
  end

  def install
    bin.install "cbox"
  end

  test do
    assert_match "cbox", shell_output("#{bin}/cbox --version")
  end
end
