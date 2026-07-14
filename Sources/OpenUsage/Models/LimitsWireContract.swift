import Foundation

struct LimitsWireEnvelope: Decodable {
    static let currentSchema = "openusage.limits.v1"

    let schema: String
    let providerId: String
    let fetchedAt: String
    let expiresAt: String
    let resources: [LimitsWireResource]

    func validate() throws {
        guard schema == Self.currentSchema else { throw ProviderWireContractError.unsupportedSchema(schema) }
        try ProviderWireValidation.identifier(providerId, field: "providerId")
        try ProviderWireValidation.timestamp(fetchedAt, field: "fetchedAt")
        try ProviderWireValidation.timestamp(expiresAt, field: "expiresAt")
        let formatter = ISO8601DateFormatter()
        guard let fetchedDate = formatter.date(from: fetchedAt),
              let expiryDate = formatter.date(from: expiresAt),
              expiryDate.timeIntervalSince(fetchedDate) == 5 * 60 else {
            throw ProviderWireContractError.invalid("limits expiry must be exactly five minutes")
        }
        guard resources.count <= 128 else {
            throw ProviderWireContractError.invalid("too many limit resources")
        }
        var ids = Set<String>()
        for resource in resources {
            try resource.validate()
            guard ids.insert(resource.id).inserted else {
                throw ProviderWireContractError.invalid("duplicate limits resource")
            }
        }
    }
}

struct LimitsWireResource: Decodable {
    let id: String
    let label: String
    let kind: LimitsWireKind
    let source: LimitsWireSource
    let used: Double?
    let limit: Double?
    let resetsAt: String?

    func validate() throws {
        try ProviderWireValidation.identifier(id, field: "resource id")
        try ProviderWireValidation.text(label, field: "resource label")
        if let used { try ProviderWireValidation.number(used, field: "resource used") }
        if let limit { try ProviderWireValidation.number(limit, field: "resource limit") }
        if let resetsAt { try ProviderWireValidation.timestamp(resetsAt, field: "resource resetsAt") }
    }
}

enum LimitsWireKind: String, Decodable {
    case rateLimit, credits, balance, spend
}

enum LimitsWireSource: String, Decodable {
    case providerApi, localHistory, estimated
}
