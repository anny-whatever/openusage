import Foundation
import Testing
@testable import OpenUsage

struct ProviderWireContractTests {
    @Test func validSnapshotFixtureCoversEveryMetricKind() throws {
        let envelope = try decode(ProviderWireEnvelope.self, fixture: "snapshot-valid.json")

        try envelope.validate()
        #expect(envelope.snapshot.lines.count == 5)
    }

    @Test func malformedSnapshotBoundariesFailLoudly() throws {
        for fixture in [
            "snapshot-invalid-negative.json",
            "snapshot-invalid-date.json",
            "snapshot-invalid-duplicate.json"
        ] {
            let envelope = try decode(ProviderWireEnvelope.self, fixture: fixture)
            #expect(throws: ProviderWireContractError.self) {
                try envelope.validate()
            }
        }
    }

    @Test func limitsFixturePreservesRealZeroAndAbsentLimit() throws {
        let envelope = try decode(LimitsWireEnvelope.self, fixture: "limits-valid.json")

        try envelope.validate()
        #expect(envelope.resources[1].used == 0)
        #expect(envelope.resources[1].limit == nil)
    }

    @Test func malformedLimitsShapeFailsDecoding() throws {
        #expect(throws: DecodingError.self) {
            _ = try decode(LimitsWireEnvelope.self, fixture: "limits-invalid-missing.json")
        }
    }

    private func decode<Value: Decodable>(_ type: Value.Type, fixture: String) throws -> Value {
        let testFile = URL(fileURLWithPath: #filePath)
        let repository = testFile
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let url = repository
            .appendingPathComponent("Tests/Fixtures/ProviderParity/v1", isDirectory: true)
            .appendingPathComponent(fixture)
        return try JSONDecoder().decode(type, from: Data(contentsOf: url))
    }
}
