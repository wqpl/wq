import XCTest
import SwiftTreeSitter
import TreeSitterWq

final class TreeSitterWqTests: XCTestCase {
    func testCanLoadGrammar() throws {
        let parser = Parser()
        let language = Language(language: tree_sitter_wq())
        XCTAssertNoThrow(try parser.setLanguage(language),
                         "Error loading wq grammar")
    }
}
