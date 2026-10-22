// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

import Foundation

@main
struct NilxoneCoreBindingSmoke {
    static func main() {
        precondition(validatePubDress(value: "0x0sky") == "valid")
        precondition(validatePubDress(value: "0x0Sky") == "valid")
        precondition(validatePubDress(value: "0xgsky") == "invalid_discriminator")
        precondition(subscriptionStatus(snapshot: "null", owner: "0x0sky", nowMs: "200") == "{\"kind\":\"unknown\"}")
        let subscription = #"{"bond_pub_dress":"0x0sky","checked_at_ms":"100","refresh_after_ms":"400","entitlement":{"tier":"exclusive","starts_at_ms":"150","expires_at_ms":"300"}}"#
        let subscriptionResult = subscriptionStatus(snapshot: subscription, owner: "0x0sky", nowMs: "200")
        let status = try! JSONSerialization.jsonObject(with: Data(subscriptionResult.utf8)) as! [String: String]
        precondition(status["kind"] == "active" && status["tier"] == "exclusive" && status["valid_until_ms"] == "300")
        precondition(contractVersion() == "0.1.0")
        precondition(fixtureCorpusVersion() == "0.1.0")
        precondition(
            fixtureCorpusDigest()
                == "sha256_d8524ee7a22aa07164362afb4098cf37404f61ab45fcfd48aab2de2fe9016009"
        )
        precondition(
            businessPointView(state: "{}", nowS: "1", organizationVerified: true)
                == "{\"error\":\"invalid\",\"ok\":false}"
        )
        precondition(
            applyBusinessPointCommand(state: "", command: "{\"op\":\"withdraw\"}", nowS: "1")
                == "{\"error\":\"invalid\",\"ok\":false}"
        )
    }
}
