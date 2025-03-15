import React from 'react';
import { BrowserRouter as Router, Route, Link, Switch } from 'react-router-dom';
import { Navbar, Nav } from 'react-bootstrap';
import Website from './layoutDiamond/Website';
import MiniApp from './layoutDiamond/MiniApp';
import Extension from './layoutDiamond/Extension';
import Home from './Home';
import NFTMarket from './NFTMarket';
import Exchange from './Exchange';
import Proxy from './Proxy';
import Farm from './Farm';
import Node from './adminJET_DASHBOARD/Node';
import Balance from './Balance';
import Snipebot from './Snipebot';
import Staking from './Staking';
import Mission from './Mission';
import NodeKey from './NodeKey';
import stats from './stats';
import miningWallet from './miningWallet';
import ProxyAdmin from './adminJET_DASHBOARD/ProxyAdmin';
import NFTAdmin from './adminJET_DASHBOARD/NFTAdmin';
import UserAdmin from './adminJET_DASHBOARD/UserAdmin';
import StatsAdmin from './adminJET_DASHBOARD/StatsAdmin';

const App = () => {
    const [platform, setPlatform] = React.useState('website');

    const Layout = platform === 'website' ? Website : platform === 'miniapp' ? MiniApp : Extension;

    return (
        <Router>
            <Navbar bg="dark" variant="dark" expand="lg">
                <Navbar.Brand>Diamond</Navbar.Brand>
                <Navbar.Toggle />
                <Navbar.Collapse>
                    <Nav className="mr-auto">
                        <Nav.Link as={Link} to="/">Bandwidth/Storage</Nav.Link>
                        <Nav.Link as={Link} to="/nft">NFT Marketplace</Nav.Link>
                        <Nav.Link as={Link} to="/exchange">Exchange</Nav.Link>
                        <Nav.Link as={Link} to="/proxy">Proxy</Nav.Link>
                        <Nav.Link as={Link} to="/farm">Farm</Nav.Link>
                        <Nav.Link as={Link} to="/node">Node</Nav.Link>
                        <Nav.Link as={Link} to="/balance">Balance</Nav.Link>
                        <Nav.Link as={Link} to="/snipebot">Snipebot</Nav.Link>
                        <Nav.Link as={Link} to="/staking">Staking</Nav.Link>
                        <Nav.Link as={Link} to="/mission">Mission</Nav.Link>
                        <Nav.Link as={Link} to="/nodekey">Node Key</Nav.Link>
                        <Nav.Link as={Link} to="/stats">Stats</Nav.Link>
                        <Nav.Link as={Link} to="/miningwallet">Mining Wallet</Nav.Link>
                    </Nav>
                    <Nav>
                        <Nav.Link onClick={() => setPlatform('website')}>Website</Nav.Link>
                        <Nav.Link onClick={() => setPlatform('miniapp')}>MiniApp</Nav.Link>
                        <Nav.Link onClick={() => setPlatform('extension')}>Extension</Nav.Link>
                    </Nav>
                </Navbar.Collapse>
            </Navbar>
            <Layout>
                <Switch>
                    <Route exact path="/" component={Home} />
                    <Route path="/nft" component={NFTMarket} />
                    <Route path="/exchange" component={Exchange} />
                    <Route path="/proxy" component={Proxy} />
                    <Route path="/farm" component={Farm} />
                    <Route path="/node" component={Node} />
                    <Route path="/balance" component={Balance} />
                    <Route path="/snipebot" component={Snipebot} />
                    <Route path="/staking" component={Staking} />
                    <Route path="/mission" component={Mission} />
                    <Route path="/nodekey" component={NodeKey} />
                    <Route path="/stats" component={stats} />
                    <Route path="/miningwallet" component={miningWallet} />
                    <Route path="/admin/proxy" component={ProxyAdmin} />
                    <Route path="/admin/nft" component={NFTAdmin} />
                    <Route path="/admin/users" component={UserAdmin} />
                    <Route path="/admin/stats" component={StatsAdmin} />
                </Switch>
            </Layout>
        </Router>
    );
};

export default App;