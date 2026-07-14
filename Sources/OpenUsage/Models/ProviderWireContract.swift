import Foundation

enum ProviderWireContractError: Error, Equatable {
    case unsupportedSchema(String)
    case invalid(String)
}

struct ProviderWireEnvelope: Decodable {
    static let currentSchema = "openusage.provider-snapshot.v1"

    let schema: String
    let snapshot: ProviderWireSnapshot

    func validate() throws {
        guard schema == Self.currentSchema else { throw ProviderWireContractError.unsupportedSchema(schema) }
        try snapshot.validate()
    }
}

struct ProviderWireSnapshot: Decodable {
    private static let maximumMetricLines = 128
    let providerId: String
    let displayName: String
    let plan: String?
    let lines: [ProviderWireMetricLine]
    let refreshedAt: String
    let usageHistory: ProviderWireUsageHistory?
    let warning: String?
    let errorCategory: String?

    func validate() throws {
        try ProviderWireValidation.identifier(providerId, field: "providerId")
        try ProviderWireValidation.text(displayName, field: "displayName")
        try ProviderWireValidation.timestamp(refreshedAt, field: "refreshedAt")
        if let plan { try ProviderWireValidation.text(plan, field: "plan") }
        if let warning { try ProviderWireValidation.text(warning, field: "warning") }
        guard lines.count <= Self.maximumMetricLines else {
            throw ProviderWireContractError.invalid("too many metric lines")
        }
        if let errorCategory {
            let categories = Set([
                "not_logged_in", "auth_expired", "auth_invalid", "credential_access", "network",
                "decoding", "http_4xx", "http_5xx", "rate_limited", "not_available", "other"
            ])
            guard categories.contains(errorCategory) else {
                throw ProviderWireContractError.invalid("unsupported errorCategory")
            }
        }

        var labels = Set<String>()
        for line in lines {
            try line.validate()
            guard labels.insert(line.label).inserted else {
                throw ProviderWireContractError.invalid("duplicate metric label")
            }
        }
        try usageHistory?.validate()
    }
}

enum ProviderWireMetricLine: Decodable {
    case text(label: String, value: String, colorHex: String?, subtitle: String?)
    case values(
        label: String,
        values: [ProviderWireMetricValue],
        colorHex: String?,
        expiriesAt: [String],
        unknownModels: [String]
    )
    case progress(
        label: String,
        used: Double,
        limit: Double,
        format: ProviderWireProgressFormat,
        resetsAt: String?,
        periodDurationMs: UInt64?,
        colorHex: String?
    )
    case badge(label: String, text: String, colorHex: String?, subtitle: String?)
    case chart(label: String, points: [ProviderWireChartPoint])

    var label: String {
        switch self {
        case .text(let label, _, _, _), .values(let label, _, _, _, _),
             .progress(let label, _, _, _, _, _, _), .badge(let label, _, _, _), .chart(let label, _): label
        }
    }

    private enum CodingKeys: String, CodingKey {
        case type, label, value, values, used, limit, format, resetsAt, periodDurationMs
        case text, points, expiriesAt, unknownModels, colorHex, subtitle
    }

    private enum LineType: String, Decodable {
        case text, values, progress, badge, chart
    }

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        let type = try values.decode(LineType.self, forKey: .type)
        let label = try values.decode(String.self, forKey: .label)
        switch type {
        case .text:
            self = .text(
                label: label,
                value: try values.decode(String.self, forKey: .value),
                colorHex: try values.decodeIfPresent(String.self, forKey: .colorHex),
                subtitle: try values.decodeIfPresent(String.self, forKey: .subtitle)
            )
        case .values:
            self = .values(
                label: label,
                values: try values.decode([ProviderWireMetricValue].self, forKey: .values),
                colorHex: try values.decodeIfPresent(String.self, forKey: .colorHex),
                expiriesAt: try values.decodeIfPresent([String].self, forKey: .expiriesAt) ?? [],
                unknownModels: try values.decodeIfPresent([String].self, forKey: .unknownModels) ?? []
            )
        case .progress:
            self = .progress(
                label: label,
                used: try values.decode(Double.self, forKey: .used),
                limit: try values.decode(Double.self, forKey: .limit),
                format: try values.decode(ProviderWireProgressFormat.self, forKey: .format),
                resetsAt: try values.decodeIfPresent(String.self, forKey: .resetsAt),
                periodDurationMs: try values.decodeIfPresent(UInt64.self, forKey: .periodDurationMs),
                colorHex: try values.decodeIfPresent(String.self, forKey: .colorHex)
            )
        case .badge:
            self = .badge(
                label: label,
                text: try values.decode(String.self, forKey: .text),
                colorHex: try values.decodeIfPresent(String.self, forKey: .colorHex),
                subtitle: try values.decodeIfPresent(String.self, forKey: .subtitle)
            )
        case .chart:
            self = .chart(label: label, points: try values.decode([ProviderWireChartPoint].self, forKey: .points))
        }
    }

    func validate() throws {
        try ProviderWireValidation.text(label, field: "metric label")
        switch self {
        case .text(_, let value, let colorHex, let subtitle), .badge(_, let value, let colorHex, let subtitle):
            try ProviderWireValidation.text(value, field: "metric value")
            try ProviderWireValidation.color(colorHex)
            if let subtitle { try ProviderWireValidation.text(subtitle, field: "subtitle") }
        case .values(_, let values, let colorHex, let expiriesAt, let unknownModels):
            guard !values.isEmpty, values.count <= 8 else {
                throw ProviderWireContractError.invalid("values count is invalid")
            }
            guard expiriesAt.count <= 64, unknownModels.count <= 64 else {
                throw ProviderWireContractError.invalid("metric metadata exceeds its fixed bound")
            }
            try ProviderWireValidation.color(colorHex)
            try values.forEach { try $0.validate() }
            try expiriesAt.forEach { try ProviderWireValidation.timestamp($0, field: "expiriesAt") }
            try unknownModels.forEach { try ProviderWireValidation.text($0, field: "unknownModels") }
        case .progress(_, let used, let limit, let format, let resetsAt, let periodDurationMs, let colorHex):
            try ProviderWireValidation.number(used, field: "used")
            try ProviderWireValidation.number(limit, field: "limit")
            guard limit > 0, used <= limit else { throw ProviderWireContractError.invalid("invalid progress range") }
            try format.validate()
            try ProviderWireValidation.color(colorHex)
            if let resetsAt { try ProviderWireValidation.timestamp(resetsAt, field: "resetsAt") }
            if let periodDurationMs, periodDurationMs == 0 {
                throw ProviderWireContractError.invalid("periodDurationMs must be positive")
            }
        case .chart(_, let points):
            guard points.count <= 31 else { throw ProviderWireContractError.invalid("chart exceeds history window") }
            try points.forEach { try $0.validate() }
        }
    }
}

struct ProviderWireMetricValue: Decodable {
    let number: Double
    let kind: MetricKind
    let label: String?
    let estimated: Bool

    func validate() throws {
        try ProviderWireValidation.number(number, field: "number")
        if kind == .percent, number > 100 { throw ProviderWireContractError.invalid("percent exceeds 100") }
        if let label { try ProviderWireValidation.text(label, field: "value label") }
    }
}

enum ProviderWireProgressFormat: Decodable {
    case percent, dollars, count(suffix: String)

    private enum CodingKeys: String, CodingKey { case kind, suffix }
    private enum Kind: String, Decodable { case percent, dollars, count }

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        switch try values.decode(Kind.self, forKey: .kind) {
        case .percent: self = .percent
        case .dollars: self = .dollars
        case .count:
            self = .count(suffix: try values.decode(String.self, forKey: .suffix))
        }
    }

    func validate() throws {
        if case .count(let suffix) = self {
            try ProviderWireValidation.text(suffix, field: "count suffix")
        }
    }
}

struct ProviderWireChartPoint: Decodable {
    let value: Double
    let label: String

    func validate() throws {
        guard days.count <= 31 else { throw ProviderWireContractError.invalid("history exceeds 31 days") }
        try ProviderWireValidation.number(value, field: "chart point value")
        try ProviderWireValidation.text(label, field: "chart point label")
    }
}

struct ProviderWireUsageHistory: Decodable {
    let days: [ProviderWireUsageDay]

    func validate() throws {
        var dates = Set<String>()
        for day in days {
            try day.validate()
            guard dates.insert(day.date).inserted else {
                throw ProviderWireContractError.invalid("duplicate history day")
            }
        }
    }
}

struct ProviderWireUsageDay: Decodable {
    let date: String
    let value: Double

    func validate() throws {
        try ProviderWireValidation.day(date, field: "history date")
        try ProviderWireValidation.number(value, field: "history value")
    }
}

enum ProviderWireValidation {
    private static let maximumTextBytes = 4096

    static func text(_ value: String, field: String) throws {
        guard !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              value.utf8.count <= maximumTextBytes else {
            throw ProviderWireContractError.invalid("\(field) must not be empty")
        }
    }

    static func identifier(_ value: String, field: String) throws {
        let allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyz0123456789-")
        guard !value.isEmpty, value.count <= 64, value.unicodeScalars.allSatisfy(allowed.contains) else {
            throw ProviderWireContractError.invalid("\(field) is invalid")
        }
    }

    static func number(_ value: Double, field: String) throws {
        guard value.isFinite, value >= 0 else {
            throw ProviderWireContractError.invalid("\(field) must be finite and non-negative")
        }
    }

    static func color(_ value: String?) throws {
        guard let value else { return }
        let hex = value.dropFirst()
        guard value.count == 7, value.first == "#", hex.allSatisfy({ $0.isHexDigit }) else {
            throw ProviderWireContractError.invalid("colorHex must use #RRGGBB")
        }
    }

    static func timestamp(_ value: String, field: String) throws {
        guard ISO8601DateFormatter().date(from: value) != nil else {
            throw ProviderWireContractError.invalid("\(field) must be RFC 3339")
        }
    }

    static func day(_ value: String, field: String) throws {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .iso8601)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd"
        formatter.isLenient = false
        guard let date = formatter.date(from: value), formatter.string(from: date) == value else {
            throw ProviderWireContractError.invalid("\(field) must use YYYY-MM-DD")
        }
    }
}
