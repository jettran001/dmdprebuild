class NFT {
    static async getAll(farmingContract) {
        return await farmingContract.methods.getAllNFTs().call();
    }

    static async mint(farmingContract, uri) {
        const tx = await farmingContract.methods.mintNFT(uri, fastWeb3.utils.toWei('1', 'ether')).send({ from: process.env.OWNER_ADDRESS });
        return tx;
    }

    static async buy(farmingContract, nftId, wallet) {
        const nft = await farmingContract.methods.nfts(nftId).call();
        if (!nft.forSale) throw new Error('Not for sale');
        await farmingContract.methods.buyNFT(nftId).send({ from: wallet });
    }
}

module.exports = NFT;