// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

import Foundation

@main
struct Ox1CoreBindingSmoke {
    static func main() {
        precondition(contractVersion() == "0.1.0")
        precondition(fixtureCorpusVersion() == "0.1.0")
        precondition(
            fixtureCorpusDigest()
                == "sha256_d8524ee7a22aa07164362afb4098cf37404f61ab45fcfd48aab2de2fe9016009"
        )
    }
}
