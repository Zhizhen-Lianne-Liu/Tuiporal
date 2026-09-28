class Tuiporal < Formula
  desc "Terminal UI for Temporal workflows"
  homepage "https://github.com/Zhizhen-Lianne-Liu/Tuiporal"
  url "https://github.com/Zhizhen-Lianne-Liu/Tuiporal/archive/78bd6e724f534c5cd1068746837ba9d664e75034.tar.gz"
  version "0.1.0"
  revision 1
  sha256 "3aa08a8e498dbb028427893049a8f4773a1d6e7182500938fe52e18544cff480"
  license "Apache-2.0"

  depends_on "protobuf" => :build
  depends_on "rust" => :build

  # GitHub source archives omit submodules. Bundle the pinned Temporal API
  # separately so the build script can generate the gRPC client bindings.
  resource "temporal-api" do
    url "https://github.com/temporalio/api/archive/1c27468c756fc8abc800235c33b74c6eba638c88.tar.gz"
    sha256 "eb13b15d0c04fa5698adc71e72c53a0c8376f78126e468e161aa3684446df19e"
  end

  def fetch
    ENV.prepend_path "PATH", formula_opt_bin("rust")
    system "cargo", "fetch", "--locked"
  end

  def install
    ENV.prepend_path "PATH", formula_opt_bin("rust")
    ENV.prepend_path "PATH", formula_opt_bin("protobuf")
    (buildpath/"proto/temporal-api").mkpath
    resource("temporal-api").stage do
      (buildpath/"proto/temporal-api").install Dir["*"]
    end
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "Browse Temporal workflows", shell_output("#{bin}/tuiporal --help")
  end
end
