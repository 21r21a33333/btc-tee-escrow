config{
network
cache<sha(alice_pubkey,bob_pubkey)>
}

---

// user facing structs
swap{
alice_pubkey
bob_pubkey
alice_wallet_addr
bob_wallet_addr
challanges : vec ( game_presentation_struct : serde_json::value )
}

---

// internal memmory
game_state<game_id , coinflip_challenge>

challenge{
swap_id : swap-id
alice_sacp : validated sacp // without a witness from bob
bob_sacp :validated sacp // without a witness from alice
game_state : random game state // in coin filp this is either head or tail to indicate the winner
}

-- design

routes :
/swap : creates a new order struct - returns the order struct
/swap/:swapid : returns current order struct

/game :

- req :
  alice_sacp
  bob_sacp
- response :
  winner-sacp

/game/:id 

swap btc-addr generation
alice addr : tapscript(no-internal-key , scripts (mulitsig(alicesig,bobsig)) , script(timelock , alicesig))
bob-addr : tapscript((no-internal-key , scripts (mulitsig(alicesig,bobsig))) , script(timelock , alicesig))
