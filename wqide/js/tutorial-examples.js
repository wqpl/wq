export function parseExampleContract(raw) {
  if (!raw) return null;
  let contract;
  try {
    contract = JSON.parse(raw);
  } catch {
    return null;
  }
  return contract && !Array.isArray(contract) && typeof contract === "object"
    ? contract
    : null;
}

export function exampleOutcome(contract, outcome) {
  const expectedError = contract?.expect?.error;
  if (expectedError) {
    if (!outcome.errorKind) {
      return {
        state: "mismatch",
        heading: `Expected ${expectedError} error, but the example succeeded`,
      };
    }
    if (outcome.errorKind === expectedError) {
      return { state: "match", heading: "Expected error" };
    }
    return {
      state: "mismatch",
      heading: `Expected ${expectedError} error, got ${outcome.errorKind}`,
    };
  }

  const expectedValue = contract?.expect?.value;
  if (expectedValue !== undefined) {
    if (outcome.value === String(expectedValue)) {
      return { state: "match", heading: "Result" };
    }
    return {
      state: "mismatch",
      heading: `Expected ${expectedValue}, got ${outcome.value ?? "an error"}`,
    };
  }

  return {
    state: outcome.errorKind ? "error" : "plain",
    heading: outcome.errorKind ? "Error" : "Result",
  };
}

export function exampleHeaderLabel(lang, contract) {
  if (contract?.file) return contract.file;
  if (contract?.role === "syntax") return `${lang} | syntax example`;
  if (contract?.expect?.error) {
    return `${lang} | expected ${contract.expect.error} error`;
  }
  return lang;
}
