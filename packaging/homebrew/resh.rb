# Homebrew formula for resh. It lives in the tap repository
# https://github.com/wongjiahau/homebrew-resh as Formula/resh.rb; this copy is the
# source of truth. After tagging a release run `packaging/homebrew/release.sh <tag>`.
class Resh < Formula
  desc "Readable shell: typed, anyfix commands and wrappers for CLI tools"
  homepage "https://github.com/wongjiahau/resh"
  url "https://github.com/wongjiahau/resh/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_THE_TAG_TARBALL"
  head "https://github.com/wongjiahau/resh.git", branch: "master"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  def caveats
    <<~EOS
      Wrappers (fd, git, herdr, rg, sed) are built in: use `IMPORT git;`.
      Your own wrappers go in ~/.config/resh/lib (see `resh libs`).
      To let Claude Code write wrappers for your tools, run:
        resh skill install
    EOS
  end

  test do
    assert_match "git", shell_output("#{bin}/resh libs")
    (testpath/"t.resh").write("IMPORT git;\n")
    system bin/"resh", "check", testpath/"t.resh"
  end
end
