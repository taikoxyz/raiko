import unittest

from stress_shasta_proposal import (
    extract_proof_from_response_data,
    generate_shasta_post_data,
    get_contract_event_logs,
    is_completed_response_data,
    shasta_proof_endpoint,
)


class ShastaProofApiVersionTest(unittest.TestCase):
    def test_v3_payload_and_endpoint_remain_unchanged(self):
        proposal = {
            "proposal_id": 42,
            "l1_inclusion_block_number": 100,
            "l2_block_numbers": [200, 201, 202],
            "checkpoint": None,
            "last_anchor_block_number": 199,
        }

        payload = generate_shasta_post_data(
            [proposal],
            proof_type="native",
            aggregate=False,
            api_version="v3",
        )

        self.assertEqual(
            shasta_proof_endpoint("http://localhost:8080", "v3"),
            "http://localhost:8080/v3/proof/batch/shasta",
        )
        self.assertEqual(payload["proposals"][0]["l2_block_numbers"], [200, 201, 202])
        self.assertIn("blob_proof_type", payload)
        self.assertIn("native", payload)

    def test_v4_payload_uses_proposal_ranges(self):
        proposal = {
            "proposal_id": 42,
            "l1_inclusion_block_number": 100,
            "l2_block_numbers": [200, 201, 202],
            "checkpoint": None,
            "last_anchor_block_number": 199,
        }

        payload = generate_shasta_post_data(
            [proposal],
            proof_type="sgx",
            aggregate=False,
            api_version="v4",
        )

        self.assertEqual(
            shasta_proof_endpoint("http://localhost:8080/", "v4"),
            "http://localhost:8080/v4/proof/proposal",
        )
        self.assertEqual(payload["proof_type"], "sgx")
        self.assertEqual(payload["proposals"][0]["l2_block_number_start"], 200)
        self.assertEqual(payload["proposals"][0]["l2_block_number_end"], 202)
        self.assertNotIn("l2_block_numbers", payload["proposals"][0])
        self.assertNotIn("blob_proof_type", payload)
        self.assertNotIn("native", payload)
        self.assertNotIn("sgx", payload)

    def test_extract_proof_accepts_v3_object_and_v4_string(self):
        self.assertEqual(
            extract_proof_from_response_data({"proof": {"proof": "0xabc"}}),
            "0xabc",
        )
        self.assertEqual(
            extract_proof_from_response_data({"proof": "0xdef"}),
            "0xdef",
        )

    def test_completed_status_counts_as_done_when_native_proof_is_null(self):
        self.assertTrue(is_completed_response_data({"status": "completed", "proof": None}))
        self.assertTrue(is_completed_response_data({"proof": {"proof": "0xabc"}}))

    def test_get_contract_event_logs_falls_back_to_web3_v6_keywords(self):
        class DummyEvent:
            def __init__(self):
                self.calls = []

            def get_logs(self, **kwargs):
                self.calls.append(kwargs)
                if "from_block" in kwargs:
                    raise TypeError("unexpected keyword argument 'from_block'")
                return ["log"]

        event = DummyEvent()

        self.assertEqual(get_contract_event_logs(event, 1, 2), ["log"])
        self.assertEqual(
            event.calls,
            [{"from_block": 1, "to_block": 2}, {"fromBlock": 1, "toBlock": 2}],
        )


if __name__ == "__main__":
    unittest.main()
